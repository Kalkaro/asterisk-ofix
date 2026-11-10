//! Steam names, covers (600x900) and heroes, from Steam's local library cache, our own
//! cache, or the Steam CDN, in that order.

use crate::paths::Paths;
use image::RgbaImage;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A game's store artwork and name; any part may be missing.
#[derive(Clone, Debug, Default)]
pub struct Art {
    pub name: String,
    /// Wide banner (library_hero).
    pub hero: Option<Arc<RgbaImage>>,
    /// 600x900 portrait capsule, used for the library cards.
    pub cover: Option<Arc<RgbaImage>>,
}

impl Art {
    fn is_empty(&self) -> bool {
        self.name.is_empty() && self.hero.is_none() && self.cover.is_none()
    }
    fn is_complete(&self) -> bool {
        !self.name.is_empty() && self.hero.is_some() && self.cover.is_some()
    }
}

/// The Steam endpoints; tests point them at a local server.
#[derive(Clone, Debug)]
pub struct Endpoints {
    pub store_api: String,
    pub asset_base: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Endpoints {
            store_api: "https://api.steampowered.com/IStoreBrowseService/GetItems/v1".into(),
            asset_base: "https://shared.akamai.steamstatic.com/store_item_assets/".into(),
        }
    }
}

#[derive(Clone)]
pub struct ArtStore(Arc<Inner>);

struct Inner {
    /// Where Steam keeps its library cache; Steam may be found after the store is made.
    steam_root: Mutex<Option<PathBuf>>,
    cache: PathBuf,
    endpoints: Endpoints,
    art: Mutex<HashMap<u32, Art>>,
    requested: Mutex<HashSet<u32>>,
}

/// False for IDs with no store page of their own (0, Spacewar).
pub fn has_art(id: u32) -> bool {
    id != 0 && id != 480
}

impl ArtStore {
    pub fn new(paths: &Paths, steam_root: Option<&Path>, endpoints: Endpoints) -> ArtStore {
        ArtStore(Arc::new(Inner {
            steam_root: Mutex::new(steam_root.map(Path::to_path_buf)),
            cache: paths.cache_dir().join("art"),
            endpoints,
            art: Mutex::default(),
            requested: Mutex::default(),
        }))
    }

    /// Points the store at the Steam found now (or none). Art loaded already stays; apps whose
    /// art isn't complete are asked for again, so the next `load` looks in that Steam's
    /// library cache.
    pub fn set_steam_root(&self, root: Option<&Path>) {
        let root = root.map(Path::to_path_buf);
        let mut current = lock(&self.0.steam_root);
        if *current == root {
            return;
        }
        *current = root;
        let art = lock(&self.0.art);
        lock(&self.0.requested).retain(|id| art.get(id).is_some_and(Art::is_complete));
    }

    /// What's loaded so far for `id`.
    pub fn get(&self, id: u32) -> Art {
        lock(&self.0.art).get(&id).cloned().unwrap_or_default()
    }

    /// Fetches artwork for ids not asked for before, on a background thread, calling `done`
    /// there for every id that got any.
    pub fn load(&self, ids: &[u32], done: impl Fn(u32) + Send + 'static) -> Option<std::thread::JoinHandle<()>> {
        let todo: Vec<u32> = {
            let mut requested = lock(&self.0.requested);
            ids.iter().copied().filter(|&id| has_art(id) && requested.insert(id)).collect()
        };
        if todo.is_empty() {
            return None;
        }
        let inner = self.0.clone();
        Some(std::thread::spawn(move || inner.fetch(&todo, &done)))
    }

    #[cfg(test)]
    /// The cached file for one of an app's images ("cover.jpg", "hero.jpg", "name.txt").
    pub fn cache_path(&self, id: u32, file: &str) -> PathBuf {
        self.0.cache.join(format!("{id}_{file}"))
    }
}

/// How many images download at once.
const DOWNLOAD_WORKERS: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    // Covers first: the library's cards show them.
    Cover,
    Hero,
}

impl Kind {
    fn file(self) -> &'static str {
        match self {
            Kind::Cover => "cover.jpg",
            Kind::Hero => "hero.jpg",
        }
    }

    fn max_width(self) -> u32 {
        match self {
            Kind::Cover => 600,
            Kind::Hero => 1920,
        }
    }
}

/// One image to download.
struct Download {
    id: u32,
    kind: Kind,
    url: String,
}

impl Inner {
    fn fetch(&self, ids: &[u32], done: &dyn Fn(u32)) {
        let mut missing = Vec::new();
        for &id in ids {
            // Asked again (Steam was found since), what was loaded before stays.
            let mut art = lock(&self.art).get(&id).cloned().unwrap_or_default();
            let local = self.load_local(id);
            if art.name.is_empty() {
                art.name = local.name;
            }
            art.hero = art.hero.or(local.hero);
            art.cover = art.cover.or(local.cover);
            if !art.is_complete() {
                missing.push(id);
            }
            self.store(id, art, done);
        }
        if missing.is_empty() {
            return;
        }
        let Ok(assets) = self.store_assets(&missing) else {
            // Most likely offline: the next scan asks again.
            self.forget(&missing);
            return;
        };
        let mut downloads = Vec::new();
        for id in missing {
            let have = lock(&self.art).get(&id).cloned().unwrap_or_default();
            let urls = assets.get(&id).cloned().unwrap_or_default();
            let url = |key: &str| urls.get(key).cloned().unwrap_or_default();
            let name = url("name");
            if have.name.is_empty() && !name.is_empty() {
                let path = self.path(id, "name.txt");
                let _ = std::fs::create_dir_all(&self.cache).and_then(|_| std::fs::write(path, &name));
                self.store(id, Art { name, ..Art::default() }, done);
            }
            // An image the store has no URL for isn't asked for again: there is none.
            for (kind, have, key) in [
                (Kind::Cover, have.cover.is_some(), "library_capsule"),
                (Kind::Hero, have.hero.is_some(), "library_hero"),
            ] {
                let url = url(key);
                if !have && !url.is_empty() {
                    downloads.push(Download { id, kind, url });
                }
            }
        }
        downloads.sort_by_key(|d| d.kind);
        let failed = self.download(downloads, done);
        // The next scan asks for these again; what did arrive stays.
        self.forget(&failed);
    }

    /// Downloads the images a few at a time, storing each as it arrives. Returns the apps
    /// whose images didn't come.
    fn download(&self, downloads: Vec<Download>, done: &dyn Fn(u32)) -> Vec<u32> {
        let workers = DOWNLOAD_WORKERS.min(downloads.len());
        let queue = Mutex::new(downloads.into_iter());
        let (tx, rx) = std::sync::mpsc::channel();
        let agent = agent();
        std::thread::scope(|s| {
            for _ in 0..workers {
                let (tx, queue, agent) = (tx.clone(), &queue, &agent);
                s.spawn(move || {
                    loop {
                        let next = lock(queue).next();
                        let Some(d) = next else { break };
                        let img = self.fetch_cached(agent, &d.url, &self.path(d.id, d.kind.file()));
                        if tx.send((d.id, d.kind, img.map(|i| fit(i, d.kind.max_width())))).is_err() {
                            break;
                        }
                    }
                });
            }
            drop(tx);
            // `done` runs here, on the fetching thread, as each image lands.
            let mut failed = Vec::new();
            for (id, kind, img) in rx {
                match (img, kind) {
                    (None, _) => failed.push(id),
                    (cover @ Some(_), Kind::Cover) => self.store(id, Art { cover, ..Art::default() }, done),
                    (hero @ Some(_), Kind::Hero) => self.store(id, Art { hero, ..Art::default() }, done),
                }
            }
            failed
        })
    }

    /// Lets the next `load` ask for these apps again.
    fn forget(&self, ids: &[u32]) {
        let mut requested = lock(&self.requested);
        for id in ids {
            requested.remove(id);
        }
    }

    /// Adds what `art` has to what's loaded for `id`, keeping the rest.
    fn store(&self, id: u32, art: Art, done: &dyn Fn(u32)) {
        if art.is_empty() {
            return;
        }
        {
            let mut all = lock(&self.art);
            let have = all.entry(id).or_default();
            if !art.name.is_empty() {
                have.name = art.name;
            }
            have.hero = art.hero.or(have.hero.take());
            have.cover = art.cover.or(have.cover.take());
        }
        done(id);
    }

    fn path(&self, id: u32, file: &str) -> PathBuf {
        self.cache.join(format!("{id}_{file}"))
    }

    fn load_local(&self, id: u32) -> Art {
        let steam_root = lock(&self.steam_root).clone();
        let find = |kind: &str, steam_name: &str| {
            let mut paths = vec![self.path(id, &format!("{kind}.jpg"))];
            if let Some(root) = &steam_root {
                let lc = root.join("appcache/librarycache");
                paths.push(lc.join(id.to_string()).join(steam_name));
                paths.push(lc.join(format!("{id}_{steam_name}")));
            }
            paths.iter().find_map(|p| decode_file(p))
        };
        let name = std::fs::read_to_string(self.path(id, "name.txt")).unwrap_or_default();
        Art {
            name: name.trim().to_owned(),
            hero: find("hero", "library_hero.jpg").map(|i| fit(i, 1920)),
            cover: find("cover", "library_600x900.jpg").map(|i| fit(i, 600)),
        }
    }

    /// Asks the Steam store for each app's name and asset URLs; newer apps keep their
    /// images under hashed paths that can't be guessed.
    fn store_assets(&self, ids: &[u32]) -> Result<HashMap<u32, HashMap<String, String>>, String> {
        let input = serde_json::json!({
            "ids": ids.iter().map(|id| serde_json::json!({ "appid": id })).collect::<Vec<_>>(),
            "context": { "language": "english", "country_code": "US" },
            "data_request": { "include_assets": true },
        });
        let body = agent()
            .get(&self.endpoints.store_api)
            .query("input_json", input.to_string())
            .call()
            .and_then(|mut r| r.body_mut().with_config().limit(4 << 20).read_to_string())
            .map_err(|e| e.to_string())?;
        let out: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
        let mut urls = HashMap::new();
        for item in out["response"]["store_items"].as_array().into_iter().flatten() {
            let Some(id) = item["appid"].as_u64().and_then(|id| u32::try_from(id).ok()) else { continue };
            let mut found = HashMap::from([("name".to_owned(), item["name"].as_str().unwrap_or("").to_owned())]);
            let assets = &item["assets"];
            if let Some(format) = assets["asset_url_format"].as_str() {
                // The 2x capsule (600x900) replaces the 1x one; the hero is 1920 wide already.
                for key in ["library_hero", "library_capsule", "library_capsule_2x"] {
                    if let Some(file) = assets[key].as_str().filter(|f| !f.is_empty()) {
                        let url = format!("{}{}", self.endpoints.asset_base, format.replace("${FILENAME}", file));
                        found.insert(key.trim_end_matches("_2x").to_owned(), url);
                    }
                }
            }
            urls.insert(id, found);
        }
        Ok(urls)
    }

    /// Downloads `src` to `dst` (unless it's there already) and decodes it.
    fn fetch_cached(&self, agent: &ureq::Agent, src: &str, dst: &Path) -> Option<RgbaImage> {
        if let Some(img) = decode_file(dst) {
            return Some(img);
        }
        if src.is_empty() {
            return None;
        }
        let data = agent.get(src).call().ok()?.body_mut().with_config().limit(16 << 20).read_to_vec().ok()?;
        std::fs::create_dir_all(&self.cache).ok()?;
        let tmp = dst.with_extension("part");
        std::fs::write(&tmp, &data).ok()?;
        std::fs::rename(&tmp, dst).ok()?;
        decode_file(dst)
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(15))).build().into()
}

fn decode_file(p: &Path) -> Option<RgbaImage> {
    let data = std::fs::read(p).ok()?;
    image::load_from_memory(&data).ok().map(|i| i.to_rgba8())
}

/// Shrinks an image to at most `max_w` wide; library heroes are 3840 wide.
fn fit(img: RgbaImage, max_w: u32) -> Arc<RgbaImage> {
    if img.width() <= max_w {
        return Arc::new(img);
    }
    let h = (img.height() as u64 * max_w as u64 / img.width() as u64) as u32;
    Arc::new(image::imageops::resize(&img, max_w, h.max(1), image::imageops::FilterType::Triangle))
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
pub mod fake {
    //! A stand-in for the Steam store: GetItems and artwork, served from a local socket.

    use std::collections::{HashMap, HashSet};
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    pub struct FakeSteam {
        pub endpoints: super::Endpoints,
        /// Connections, API calls and images alike.
        pub hits: Arc<AtomicUsize>,
        pub stats: Arc<Stats>,
        behaviour: Arc<Mutex<Behaviour>>,
    }

    /// What the fake has served.
    #[derive(Default)]
    pub struct Stats {
        pub api_calls: AtomicUsize,
        /// Image requests being answered right now, and the most there ever were.
        in_flight: AtomicUsize,
        pub most_images_at_once: AtomicUsize,
    }

    /// How the fake misbehaves; tests change it as they go.
    #[derive(Default)]
    pub struct Behaviour {
        /// Images whose path contains one of these answer 500: "apps/70/" fails all of app
        /// 70's, "70/abc/library_hero" just its hero.
        pub failing: Vec<String>,
        /// Apps the store knows by name but has no images for.
        pub imageless: HashSet<u32>,
        /// How long each image takes.
        pub image_delay: Duration,
    }

    impl FakeSteam {
        pub fn behave(&self, f: impl FnOnce(&mut Behaviour)) {
            f(&mut super::lock(&self.behaviour));
        }
    }

    /// Serves apps in `names` with a name, a 2400x775 hero and a 600x900 capsule.
    pub fn start(names: &[(u32, &str)]) -> FakeSteam {
        let names: Arc<HashMap<u32, String>> = Arc::new(names.iter().map(|(id, n)| (*id, n.to_string())).collect());
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let hits = Arc::new(AtomicUsize::new(0));
        let stats = Arc::new(Stats::default());
        let behaviour = Arc::new(Mutex::new(Behaviour::default()));
        let (counter, server_stats, server_behaviour) = (hits.clone(), stats.clone(), behaviour.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                counter.fetch_add(1, Ordering::SeqCst);
                let (names, stats, behaviour) = (names.clone(), server_stats.clone(), server_behaviour.clone());
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(&stream);
                    let mut line = String::new();
                    let _ = reader.read_line(&mut line);
                    let target = line.split(' ').nth(1).unwrap_or("").to_owned();
                    let (status, body) = respond(&names, &stats, &behaviour, &target);
                    let mut out = &stream;
                    let _ =
                        write!(out, "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                    let _ = out.write_all(&body);
                });
            }
        });
        FakeSteam {
            endpoints: super::Endpoints { store_api: format!("{base}/api"), asset_base: format!("{base}/assets/") },
            hits,
            stats,
            behaviour,
        }
    }

    fn respond(
        names: &HashMap<u32, String>,
        stats: &Stats,
        behaviour: &Mutex<Behaviour>,
        target: &str,
    ) -> (&'static str, Vec<u8>) {
        let path = target.split('?').next().unwrap_or("");
        // Whatever else finds the port (another process probing it) doesn't count.
        if !path.starts_with("/assets/") && path != "/api" {
            return ("404 Not Found", Vec::new());
        }
        if path != "/api" {
            let now = stats.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            stats.most_images_at_once.fetch_max(now, Ordering::SeqCst);
            let (delay, failing) = {
                let b = super::lock(behaviour);
                (b.image_delay, b.failing.iter().any(|f| path.contains(f.as_str())))
            };
            std::thread::sleep(delay);
            let out = match path {
                _ if failing => ("500 Internal Server Error", Vec::new()),
                p if p.ends_with("library_hero.jpg") => ("200 OK", jpeg(2400, 775)),
                p if p.ends_with("library_capsule_2x.jpg") => ("200 OK", jpeg(600, 900)),
                p if p.ends_with("library_capsule.jpg") => ("200 OK", jpeg(300, 450)),
                _ => ("404 Not Found", Vec::new()),
            };
            stats.in_flight.fetch_sub(1, Ordering::SeqCst);
            return out;
        }
        stats.api_calls.fetch_add(1, Ordering::SeqCst);
        {
            let imageless = &super::lock(behaviour).imageless;
            let query = target.split_once("input_json=").map(|(_, q)| decode(q)).unwrap_or_default();
            let input: serde_json::Value = serde_json::from_str(&query).unwrap_or_default();
            let items: Vec<_> = input["ids"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|i| {
                    let id = i["appid"].as_u64()? as u32;
                    let name = names.get(&id)?;
                    if imageless.contains(&id) {
                        return Some(serde_json::json!({ "appid": id, "name": name, "assets": {} }));
                    }
                    Some(serde_json::json!({ "appid": id, "name": name, "assets": {
                        "asset_url_format": format!("steam/apps/{id}/${{FILENAME}}?t=1"),
                        "library_hero": "abc/library_hero.jpg",
                        "library_capsule": "def/library_capsule.jpg",
                        "library_capsule_2x": "def/library_capsule_2x.jpg",
                    }}))
                })
                .collect();
            let body = serde_json::json!({ "response": { "store_items": items } });
            ("200 OK", body.to_string().into_bytes())
        }
    }

    pub fn jpeg(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbImage::from_pixel(w, h, image::Rgb([0x80, 0x80, 0x80]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Jpeg).unwrap();
        out.into_inner()
    }

    fn decode(q: &str) -> String {
        let q = q.split('&').next().unwrap_or("").replace('+', " ");
        let mut out = Vec::new();
        let b = q.as_bytes();
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'%'
                && let Some(n) = q.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok())
            {
                out.push(n);
                i += 3;
                continue;
            }
            out.push(b[i]);
            i += 1;
        }
        String::from_utf8_lossy(&out).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::tempdir;
    use std::sync::atomic::Ordering;

    fn wait_for(what: &str, cond: impl Fn() -> bool) {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !cond() {
            assert!(std::time::Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn downloads_once_and_then_reads_the_cache() {
        let home = tempdir();
        let paths = Paths::under(home.path());
        let steam = fake::start(&[(1000001, "Sky Ride")]);
        let load = |endpoints: &Endpoints| {
            let store = ArtStore::new(&paths, None, endpoints.clone());
            store.load(&[1000001, 480, 0], |_| {});
            wait_for("art", || store.get(1000001).is_complete());
            store
        };
        let store = load(&steam.endpoints);
        let art = store.get(1000001);
        assert_eq!(art.name, "Sky Ride");
        let cover = art.cover.unwrap();
        assert_eq!((cover.width(), cover.height()), (600, 900), "the 2x capsule");
        assert_eq!(art.hero.unwrap().width(), 1920, "heroes are shrunk to 1920 wide");
        for f in ["cover.jpg", "hero.jpg", "name.txt"] {
            assert!(store.cache_path(1000001, f).exists(), "{f} not cached");
        }

        // A second load comes from the cache: with the store unreachable it still completes.
        // (Counting the fake store's connections instead raced with the first load's.)
        let offline =
            Endpoints { store_api: "http://127.0.0.1:9/api".into(), asset_base: "http://127.0.0.1:9/assets/".into() };
        assert_eq!(load(&offline).get(1000001).name, "Sky Ride");
    }

    #[test]
    fn skips_placeholder_ids() {
        let home = tempdir();
        let steam = fake::start(&[]);
        let store = ArtStore::new(&Paths::under(home.path()), None, steam.endpoints.clone());
        store.load(&[0, 480], |_| panic!("callback for a placeholder ID"));
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(steam.hits.load(Ordering::SeqCst), 0, "0 and Spacewar (480) have no art of their own");
    }

    /// A store request that failed (offline at startup) marked its apps as done for good, so a
    /// rescan never fetched their art.
    #[test]
    fn a_failed_store_request_is_retried() {
        let home = tempdir();
        let offline =
            Endpoints { store_api: "http://127.0.0.1:9/api".into(), asset_base: "http://127.0.0.1:9/assets/".into() };
        let store = ArtStore::new(&Paths::under(home.path()), None, offline);
        store.load(&[1000001], |_| {}).expect("a fetch").join().unwrap();
        assert!(store.load(&[1000001], |_| {}).is_some(), "asked again after the store was unreachable");
    }

    /// Steam installed while asterofix is open: once the store is pointed at it, the next load
    /// looks in its library cache, though the Steam store answered for the app already, and
    /// what was loaded before stays.
    #[test]
    fn a_steam_found_later_is_looked_in() {
        let home = tempdir();
        let paths = Paths::under(home.path());
        crate::testutil::write(&paths.cache_dir().join("art/77_name.txt"), "Old Game\n");
        let cover = paths.cache_dir().join("art/77_cover.jpg");
        crate::testutil::write(&cover, fake::jpeg(600, 900));
        let steam = fake::start(&[]);
        let store = ArtStore::new(&paths, None, steam.endpoints.clone());
        store.load(&[77], |_| {}).expect("a fetch").join().unwrap();
        assert!(store.get(77).cover.is_some() && store.get(77).hero.is_none());
        assert!(store.load(&[77], |_| {}).is_none(), "asked for once");

        std::fs::remove_file(&cover).unwrap();
        let root = home.path().join(".local/share/Steam");
        crate::testutil::write(&root.join("appcache/librarycache/77/library_hero.jpg"), fake::jpeg(1920, 620));
        store.set_steam_root(Some(&root));
        store.load(&[77], |_| {}).expect("asked again with Steam found").join().unwrap();
        let art = store.get(77);
        assert!(art.hero.is_some(), "the hero from Steam's library cache");
        assert!(art.cover.is_some(), "the cover loaded before stays");
        assert_eq!(art.name, "Old Game");
        store.set_steam_root(Some(&root));
        assert!(store.load(&[77], |_| {}).is_none(), "the same Steam again asks nothing new");
    }

    #[test]
    fn reads_cached_art_without_the_network() {
        let home = tempdir();
        let paths = Paths::under(home.path());
        crate::testutil::write(&paths.cache_dir().join("art/77_cover.jpg"), fake::jpeg(600, 900));
        crate::testutil::write(&paths.cache_dir().join("art/77_name.txt"), "Old Game\n");
        let steam = fake::start(&[]);
        let store = ArtStore::new(&paths, None, steam.endpoints.clone());
        store.load(&[77], |_| {});
        wait_for("cached art", || store.get(77).cover.is_some());
        assert_eq!(store.get(77).name, "Old Game");
    }

    /// Ids `load` handed `done`, in order.
    fn load_and_wait(store: &ArtStore, ids: &[u32]) -> Option<Vec<u32>> {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        let job = store.load(ids, move |id| lock(&log).push(id))?;
        job.join().unwrap();
        Some(lock(&seen).clone())
    }

    /// A library of 40 games made 80 image requests one after another.
    #[test]
    fn images_download_several_at_a_time() {
        let home = tempdir();
        let apps: Vec<(u32, String)> = (0..12).map(|i| (2000000 + i, format!("Game {i}"))).collect();
        let names: Vec<(u32, &str)> = apps.iter().map(|(id, n)| (*id, n.as_str())).collect();
        let steam = fake::start(&names);
        steam.behave(|b| b.image_delay = Duration::from_millis(100));
        let store = ArtStore::new(&Paths::under(home.path()), None, steam.endpoints.clone());
        let ids: Vec<u32> = apps.iter().map(|(id, _)| *id).collect();
        let seen = load_and_wait(&store, &ids).expect("a fetch");
        for id in &ids {
            assert!(store.get(*id).is_complete(), "{id} has all its art");
            assert!(seen.contains(id), "done({id}) fired");
        }
        assert_eq!(steam.stats.api_calls.load(Ordering::SeqCst), 1, "one store call for the batch");
        let most = steam.stats.most_images_at_once.load(Ordering::SeqCst);
        assert!(most > 1, "images downloaded one at a time");
        assert!(most <= DOWNLOAD_WORKERS, "{most} images at once");
    }

    /// An image that failed to download stayed missing until the app restarted.
    #[test]
    fn a_failed_image_download_is_retried() {
        let home = tempdir();
        let steam = fake::start(&[(1000001, "Sky Ride"), (1000002, "Zebra Party")]);
        steam.behave(|b| b.failing = vec!["apps/1000001/".into()]);
        let store = ArtStore::new(&Paths::under(home.path()), None, steam.endpoints.clone());
        load_and_wait(&store, &[1000001, 1000002]).expect("a fetch");
        assert_eq!(store.get(1000001).name, "Sky Ride");
        assert!(store.get(1000001).cover.is_none());
        assert!(store.get(1000002).is_complete());

        steam.behave(|b| b.failing.clear());
        let seen = load_and_wait(&store, &[1000001, 1000002]).expect("the failed app is asked for again");
        assert!(store.get(1000001).is_complete());
        assert!(!seen.contains(&1000002), "an app with all its art isn't fetched again");
        assert!(store.load(&[1000001, 1000002], |_| {}).is_none(), "nothing left to fetch");
    }

    #[test]
    fn an_app_the_store_has_no_images_for_is_not_asked_again() {
        let home = tempdir();
        let steam = fake::start(&[(1000001, "Sky Ride")]);
        steam.behave(|b| b.imageless = HashSet::from([1000001]));
        let store = ArtStore::new(&Paths::under(home.path()), None, steam.endpoints.clone());
        load_and_wait(&store, &[1000001, 1000003]).expect("a fetch");
        assert_eq!(store.get(1000001).name, "Sky Ride");
        assert!(store.load(&[1000001, 1000003], |_| {}).is_none(), "no images, or not in the store at all");
    }

    /// A retry put what it found locally in place of what was loaded, so a cover no longer in
    /// the cache went missing.
    #[test]
    fn loaded_art_survives_a_retry() {
        let home = tempdir();
        let steam = fake::start(&[(1000001, "Sky Ride")]);
        steam.behave(|b| b.failing = vec!["1000001/abc/library_hero".into()]);
        let store = ArtStore::new(&Paths::under(home.path()), None, steam.endpoints.clone());
        load_and_wait(&store, &[1000001]).expect("a fetch");
        assert!(store.get(1000001).cover.is_some() && store.get(1000001).hero.is_none());

        std::fs::remove_file(store.cache_path(1000001, "cover.jpg")).unwrap();
        steam.behave(|b| b.failing = vec!["apps/1000001/".into()]);
        load_and_wait(&store, &[1000001]).expect("the missing hero is asked for again");
        let art = store.get(1000001);
        assert!(art.cover.is_some(), "the cover was lost");
        assert_eq!(art.name, "Sky Ride");

        steam.behave(|b| b.failing.clear());
        load_and_wait(&store, &[1000001]).expect("asked for again");
        assert!(store.get(1000001).is_complete());
    }
}
