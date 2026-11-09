//! The UI's languages and their strings. The language follows the system's locale until one is
//! picked from the header's globe, which saves it in config.json.
//!
//! Every language is one `Strings`, so a string missing from one doesn't build. Templates
//! number their arguments (`{0}`, `{1}`), because word order differs between languages.

use std::fmt::Display;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Lang {
    En,
    Ru,
    Uk,
    De,
    Fr,
    Es,
    PtBr,
    Pl,
    Tr,
    ZhCn,
    Ja,
}

impl Lang {
    pub const ALL: [Lang; 11] = [
        Lang::En,
        Lang::Ru,
        Lang::Uk,
        Lang::De,
        Lang::Fr,
        Lang::Es,
        Lang::PtBr,
        Lang::Pl,
        Lang::Tr,
        Lang::ZhCn,
        Lang::Ja,
    ];

    /// As saved in config.json.
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Ru => "ru",
            Lang::Uk => "uk",
            Lang::De => "de",
            Lang::Fr => "fr",
            Lang::Es => "es",
            Lang::PtBr => "pt-BR",
            Lang::Pl => "pl",
            Lang::Tr => "tr",
            Lang::ZhCn => "zh-CN",
            Lang::Ja => "ja",
        }
    }

    pub fn from_code(code: &str) -> Option<Lang> {
        Lang::ALL.into_iter().find(|l| l.code() == code)
    }

    /// The language's name in itself, as the menu lists it.
    pub fn name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Ru => "Русский",
            Lang::Uk => "Українська",
            Lang::De => "Deutsch",
            Lang::Fr => "Français",
            Lang::Es => "Español",
            Lang::PtBr => "Português (Brasil)",
            Lang::Pl => "Polski",
            Lang::Tr => "Türkçe",
            Lang::ZhCn => "简体中文",
            Lang::Ja => "日本語",
        }
    }

    pub fn from_name(name: &str) -> Option<Lang> {
        Lang::ALL.into_iter().find(|l| l.name() == name)
    }

    /// The language of a POSIX locale (`ru_RU.UTF-8`) or a `LANGUAGE` entry (`pt_BR`, `uk`).
    fn from_locale(locale: &str) -> Option<Lang> {
        let locale = locale.split(['.', '@']).next().unwrap_or_default();
        let (lang, region) = locale.split_once(['_', '-']).unwrap_or((locale, ""));
        Some(match lang {
            "en" => Lang::En,
            "ru" => Lang::Ru,
            "uk" => Lang::Uk,
            "de" => Lang::De,
            "fr" => Lang::Fr,
            "es" => Lang::Es,
            // The only Portuguese there is.
            "pt" => Lang::PtBr,
            "pl" => Lang::Pl,
            "tr" => Lang::Tr,
            // Traditional Chinese readers get English rather than the other script.
            "zh" if !matches!(region, "TW" | "HK" | "MO") => Lang::ZhCn,
            "ja" => Lang::Ja,
            _ => return None,
        })
    }

    /// The system's language, as gettext picks one: `LC_ALL`, then `LC_MESSAGES`, then
    /// `LANG`, with `LANGUAGE`'s list ahead of them unless the locale is C. English when none
    /// of them is a language we have. `env` reads a variable; tests pass their own.
    pub fn detect(env: impl Fn(&str) -> Option<String>) -> Lang {
        let var = |k: &str| env(k).filter(|v| !v.is_empty());
        let locale = var("LC_ALL").or_else(|| var("LC_MESSAGES")).or_else(|| var("LANG")).unwrap_or_default();
        if locale.is_empty() || locale == "POSIX" || locale.starts_with("C.") || locale == "C" {
            return Lang::En;
        }
        let listed = var("LANGUAGE").unwrap_or_default();
        listed.split(':').chain([locale.as_str()]).find_map(Lang::from_locale).unwrap_or(Lang::En)
    }

    pub fn strings(self) -> &'static Strings {
        match self {
            Lang::En => &EN,
            Lang::Ru => &RU,
            Lang::Uk => &UK,
            Lang::De => &DE,
            Lang::Fr => &FR,
            Lang::Es => &ES,
            Lang::PtBr => &PT_BR,
            Lang::Pl => &PL,
            Lang::Tr => &TR,
            Lang::ZhCn => &ZH_CN,
            Lang::Ja => &JA,
        }
    }
}

/// `template` with `{0}`, `{1}`, ... replaced by `args`.
pub fn fill(template: &str, args: &[&dyn Display]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    // One pass over the template, so an argument that looks like a placeholder stays as it is.
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        rest = &rest[open..];
        let arg = rest[1..].split_once('}').and_then(|(n, _)| n.parse::<usize>().ok()).and_then(|i| args.get(i));
        match arg {
            Some(arg) => {
                out.push_str(&arg.to_string());
                rest = &rest[rest.find('}').unwrap_or(0) + 1..];
            }
            None => {
                out.push('{');
                rest = &rest[1..];
            }
        }
    }
    out + rest
}

macro_rules! strings {
    ($($(#[$doc:meta])* $field:ident,)*) => {
        pub struct Strings {
            $($(#[$doc])* pub $field: &'static str,)*
            /// "12 games", in the search bar.
            pub games: fn(usize) -> String,
        }

        impl Strings {
            /// Every string by its field's name, for the tests.
            #[cfg(test)]
            pub fn all(&self) -> Vec<(&'static str, &'static str)> {
                vec![$((stringify!($field), self.$field)),*]
            }
        }
    };
}

strings! {
    library,
    /// "{0} of {1}" games shown, while a search narrows the library.
    of,
    rescan,
    rescan_help,
    add_folder,
    add_game,
    language,
    search_placeholder,
    search,
    close_search,
    play,
    stop,
    running,
    no_games,
    no_games_body,
    no_matches,
    /// {0}: what was searched for.
    no_matches_body,
    /// {0}: the game's Steam app id.
    steam_app,
    back,
    back_help,
    folder,
    log,
    copy_command,
    launch_options,
    defaults_hint,
    no_proton,
    executable,
    no_executable,
    browse,
    steam_app_id,
    arguments,
    /// The Arguments field's placeholder.
    none,
    dll_overrides,
    environment,
    wine_prefix,
    no_steam,
    /// {0}: the game. It ended because Stop was pressed.
    stopped,
    /// {0}: the game. It ended by itself.
    exited,
    /// {0}: the game, {1}: what went wrong.
    exited_with,
    cant_save,
    already_running,
    starting_steam,
    cant_start_steam,
    /// {0}: the game, {1}: the Proton build.
    launched,
    launch_failed,
    already_closing,
    stopping,
    command_copied,
    add_folder_title,
    folder_picker_failed,
    add_game_title,
    choose_exe_title,
    windows_programs,
    no_log,
    file_picker_failed,
    install,
    install_game_title,
    install_fix_title,
    /// {0}: the game, while its installer runs.
    installing,
    /// {0}: the game, while its online fix's installer runs.
    installing_fix,
    /// {0}: the game.
    installed,
    install_failed,
    /// {0}: the game being installed.
    already_installing,
    install_title,
    install_description,
    game_installer,
    online_fix,
    not_chosen,
    /// {0}: the folder the game will be installed in.
    installs_to,
    cancel,
    remove,
}

/// Slavic plurals: one, few (2-4, not 12-14) and many.
fn slavic(n: usize, one: &str, few: &str, many: &str) -> String {
    let word = match (n % 10, n % 100) {
        (1, r) if r != 11 => one,
        (2..=4, r) if !(12..=14).contains(&r) => few,
        _ => many,
    };
    format!("{n} {word}")
}

/// Polish has "one" only for 1 itself: 21 is "21 gier".
fn polish(n: usize) -> String {
    if n == 1 { "1 gra".into() } else { slavic(n, "gier", "gry", "gier") }
}

pub static EN: Strings = Strings {
    library: "Library",
    of: "{0} of {1}",
    rescan: "Rescan",
    rescan_help: "Rescan library folders",
    add_folder: "Add folder",
    add_game: "Add game",
    language: "Language",
    search_placeholder: "Search games",
    search: "Search",
    close_search: "Close search",
    play: "Play",
    stop: "Stop",
    running: "Running",
    no_games: "No games yet",
    no_games_body: "Add a folder that contains OnlineFix games,\nor pick a game's .exe directly.",
    no_matches: "No matches",
    no_matches_body: "Nothing in your library matches \u{201c}{0}\u{201d}.",
    steam_app: "Steam app {0}",
    back: "Back",
    back_help: "Back to the library (Esc)",
    folder: "Folder",
    log: "Log",
    copy_command: "Copy command",
    launch_options: "Launch options",
    defaults_hint: "Empty fields use the detected defaults.",
    no_proton: "No Proton found",
    executable: "Executable",
    no_executable: "No executable found",
    browse: "Browse",
    steam_app_id: "Steam app ID",
    arguments: "Arguments",
    none: "none",
    dll_overrides: "DLL overrides",
    environment: "Environment",
    wine_prefix: "Wine prefix",
    no_steam: "Steam installation not found. Install Steam and log in once.",
    stopped: "{0} stopped",
    exited: "{0} exited",
    exited_with: "{0} exited: {1}",
    cant_save: "Could not save settings: {0}",
    already_running: "{0} is already running",
    starting_steam: "Steam isn't running, starting it. Press Play again once you're logged in.",
    cant_start_steam: "Could not start Steam: {0}",
    launched: "Launched {0} with {1}",
    launch_failed: "Launch failed: {0}",
    already_closing: "{0} is already closing",
    stopping: "Stopping {0}",
    command_copied: "Launch command copied",
    add_folder_title: "Add a folder of games",
    folder_picker_failed: "Could not open a folder picker: {0}",
    add_game_title: "Add a game",
    choose_exe_title: "Choose the game's executable",
    windows_programs: "Windows programs",
    no_log: "No log yet. Launch the game first.",
    file_picker_failed: "Could not open a file picker: {0}",
    install: "Install",
    install_game_title: "Choose the game\u{2019}s installer",
    install_fix_title: "Choose the game\u{2019}s online fix",
    installing: "Installing {0}\u{2026}",
    installing_fix: "Installing the online fix for {0}\u{2026}",
    installed: "Installed {0}",
    install_failed: "Install failed: {0}",
    already_installing: "Already installing {0}",
    install_title: "Install a game",
    install_description: "The game\u{2019}s installer runs silently, then its online fix.",
    game_installer: "Game installer",
    online_fix: "Online fix (optional)",
    not_chosen: "Not chosen",
    installs_to: "Installs to {0}",
    cancel: "Cancel",
    remove: "Remove",
    games: |n| if n == 1 { "1 game".into() } else { format!("{n} games") },
};

pub static RU: Strings = Strings {
    library: "Библиотека",
    of: "{0} из {1}",
    rescan: "Обновить",
    rescan_help: "Заново просканировать папки библиотеки",
    add_folder: "Добавить папку",
    add_game: "Добавить игру",
    language: "Язык",
    search_placeholder: "Поиск игр",
    search: "Поиск",
    close_search: "Закрыть поиск",
    play: "Играть",
    stop: "Остановить",
    running: "Запущена",
    no_games: "Игр пока нет",
    no_games_body: "Добавьте папку с играми OnlineFix\nили выберите .exe игры напрямую.",
    no_matches: "Ничего не найдено",
    no_matches_body: "В библиотеке нет ничего по запросу «{0}».",
    steam_app: "Приложение Steam {0}",
    back: "Назад",
    back_help: "Назад в библиотеку (Esc)",
    folder: "Папка",
    log: "Лог",
    copy_command: "Копировать команду",
    launch_options: "Параметры запуска",
    defaults_hint: "В пустых полях используются найденные значения по умолчанию.",
    no_proton: "Proton не найден",
    executable: "Исполняемый файл",
    no_executable: "Исполняемый файл не найден",
    browse: "Обзор",
    steam_app_id: "ID приложения Steam",
    arguments: "Аргументы",
    none: "нет",
    dll_overrides: "Переопределения DLL",
    environment: "Окружение",
    wine_prefix: "Префикс Wine",
    no_steam: "Steam не найден. Установите Steam и войдите хотя бы раз.",
    stopped: "Игра {0} остановлена",
    exited: "Игра {0} завершилась",
    exited_with: "Игра {0} завершилась: {1}",
    cant_save: "Не удалось сохранить настройки: {0}",
    already_running: "Игра {0} уже запущена",
    starting_steam: "Steam не запущен, запускаем его. Нажмите «Играть» ещё раз, когда войдёте.",
    cant_start_steam: "Не удалось запустить Steam: {0}",
    launched: "Игра {0} запущена через {1}",
    launch_failed: "Не удалось запустить: {0}",
    already_closing: "Игра {0} уже закрывается",
    stopping: "Останавливаем {0}",
    command_copied: "Команда запуска скопирована",
    add_folder_title: "Добавить папку с играми",
    folder_picker_failed: "Не удалось открыть выбор папки: {0}",
    add_game_title: "Добавить игру",
    choose_exe_title: "Выберите исполняемый файл игры",
    windows_programs: "Программы Windows",
    no_log: "Лога пока нет. Сначала запустите игру.",
    file_picker_failed: "Не удалось открыть выбор файла: {0}",
    install: "Установить",
    install_game_title: "Выберите установщик игры",
    install_fix_title: "Выберите онлайн-фикс игры",
    installing: "Устанавливаем {0}\u{2026}",
    installing_fix: "Устанавливаем онлайн-фикс для {0}\u{2026}",
    installed: "{0} установлена",
    install_failed: "Не удалось установить: {0}",
    already_installing: "{0} уже устанавливается",
    install_title: "Установка игры",
    install_description: "Сначала тихо запустится установщик игры, затем онлайн-фикс.",
    game_installer: "Установщик игры",
    online_fix: "Онлайн-фикс (необязательно)",
    not_chosen: "Не выбран",
    installs_to: "Будет установлена в {0}",
    cancel: "Отмена",
    remove: "Убрать",
    games: |n| slavic(n, "игра", "игры", "игр"),
};

pub static UK: Strings = Strings {
    library: "Бібліотека",
    of: "{0} з {1}",
    rescan: "Оновити",
    rescan_help: "Повторно сканувати теки бібліотеки",
    add_folder: "Додати теку",
    add_game: "Додати гру",
    language: "Мова",
    search_placeholder: "Пошук ігор",
    search: "Пошук",
    close_search: "Закрити пошук",
    play: "Грати",
    stop: "Зупинити",
    running: "Запущено",
    no_games: "Ігор поки немає",
    no_games_body: "Додайте теку з іграми OnlineFix\nабо виберіть .exe гри напряму.",
    no_matches: "Нічого не знайдено",
    no_matches_body: "У бібліотеці немає нічого за запитом «{0}».",
    steam_app: "Застосунок Steam {0}",
    back: "Назад",
    back_help: "Назад до бібліотеки (Esc)",
    folder: "Тека",
    log: "Лог",
    copy_command: "Копіювати команду",
    launch_options: "Параметри запуску",
    defaults_hint: "У порожніх полях використовуються виявлені типові значення.",
    no_proton: "Proton не знайдено",
    executable: "Виконуваний файл",
    no_executable: "Виконуваний файл не знайдено",
    browse: "Огляд",
    steam_app_id: "ID застосунку Steam",
    arguments: "Аргументи",
    none: "немає",
    dll_overrides: "Перевизначення DLL",
    environment: "Середовище",
    wine_prefix: "Префікс Wine",
    no_steam: "Steam не знайдено. Встановіть Steam і увійдіть хоча б раз.",
    stopped: "Гру {0} зупинено",
    exited: "Гра {0} завершилась",
    exited_with: "Гра {0} завершилась: {1}",
    cant_save: "Не вдалося зберегти налаштування: {0}",
    already_running: "Гра {0} вже запущена",
    starting_steam: "Steam не запущено, запускаємо його. Натисніть «Грати» ще раз, коли увійдете.",
    cant_start_steam: "Не вдалося запустити Steam: {0}",
    launched: "Гру {0} запущено через {1}",
    launch_failed: "Не вдалося запустити: {0}",
    already_closing: "Гра {0} вже закривається",
    stopping: "Зупиняємо {0}",
    command_copied: "Команду запуску скопійовано",
    add_folder_title: "Додати теку з іграми",
    folder_picker_failed: "Не вдалося відкрити вибір теки: {0}",
    add_game_title: "Додати гру",
    choose_exe_title: "Виберіть виконуваний файл гри",
    windows_programs: "Програми Windows",
    no_log: "Логу поки немає. Спочатку запустіть гру.",
    file_picker_failed: "Не вдалося відкрити вибір файлу: {0}",
    install: "Встановити",
    install_game_title: "Виберіть інсталятор гри",
    install_fix_title: "Виберіть онлайн-фікс гри",
    installing: "Встановлюємо {0}\u{2026}",
    installing_fix: "Встановлюємо онлайн-фікс для {0}\u{2026}",
    installed: "{0} встановлено",
    install_failed: "Не вдалося встановити: {0}",
    already_installing: "{0} вже встановлюється",
    install_title: "Встановлення гри",
    install_description: "Спочатку тихо запуститься інсталятор гри, потім онлайн-фікс.",
    game_installer: "Інсталятор гри",
    online_fix: "Онлайн-фікс (необов\u{2019}язково)",
    not_chosen: "Не вибрано",
    installs_to: "Буде встановлено в {0}",
    cancel: "Скасувати",
    remove: "Прибрати",
    games: |n| slavic(n, "гра", "гри", "ігор"),
};

pub static DE: Strings = Strings {
    library: "Bibliothek",
    of: "{0} von {1}",
    rescan: "Neu scannen",
    rescan_help: "Bibliotheksordner neu durchsuchen",
    add_folder: "Ordner hinzufügen",
    add_game: "Spiel hinzufügen",
    language: "Sprache",
    search_placeholder: "Spiele durchsuchen",
    search: "Suche",
    close_search: "Suche schließen",
    play: "Spielen",
    stop: "Stoppen",
    running: "Läuft",
    no_games: "Noch keine Spiele",
    no_games_body: "Füge einen Ordner mit OnlineFix-Spielen hinzu\noder wähle direkt die .exe eines Spiels.",
    no_matches: "Keine Treffer",
    no_matches_body: "Nichts in deiner Bibliothek passt zu \u{201e}{0}\u{201c}.",
    steam_app: "Steam-App {0}",
    back: "Zurück",
    back_help: "Zurück zur Bibliothek (Esc)",
    folder: "Ordner",
    log: "Log",
    copy_command: "Befehl kopieren",
    launch_options: "Startoptionen",
    defaults_hint: "Leere Felder nutzen die erkannten Standardwerte.",
    no_proton: "Kein Proton gefunden",
    executable: "Programmdatei",
    no_executable: "Keine Programmdatei gefunden",
    browse: "Durchsuchen",
    steam_app_id: "Steam-App-ID",
    arguments: "Argumente",
    none: "keine",
    dll_overrides: "DLL-Overrides",
    environment: "Umgebung",
    wine_prefix: "Wine-Präfix",
    no_steam: "Steam-Installation nicht gefunden. Installiere Steam und melde dich einmal an.",
    stopped: "{0} gestoppt",
    exited: "{0} beendet",
    exited_with: "{0} beendet: {1}",
    cant_save: "Einstellungen konnten nicht gespeichert werden: {0}",
    already_running: "{0} läuft bereits",
    starting_steam: "Steam läuft nicht und wird gestartet. Drücke erneut auf Spielen, sobald du angemeldet bist.",
    cant_start_steam: "Steam konnte nicht gestartet werden: {0}",
    launched: "{0} mit {1} gestartet",
    launch_failed: "Start fehlgeschlagen: {0}",
    already_closing: "{0} wird bereits beendet",
    stopping: "{0} wird gestoppt",
    command_copied: "Startbefehl kopiert",
    add_folder_title: "Ordner mit Spielen hinzufügen",
    folder_picker_failed: "Ordnerauswahl konnte nicht geöffnet werden: {0}",
    add_game_title: "Spiel hinzufügen",
    choose_exe_title: "Programmdatei des Spiels wählen",
    windows_programs: "Windows-Programme",
    no_log: "Noch kein Log. Starte zuerst das Spiel.",
    file_picker_failed: "Dateiauswahl konnte nicht geöffnet werden: {0}",
    install: "Installieren",
    install_game_title: "Installer des Spiels wählen",
    install_fix_title: "Online-Fix des Spiels wählen",
    installing: "{0} wird installiert\u{2026}",
    installing_fix: "Online-Fix für {0} wird installiert\u{2026}",
    installed: "{0} installiert",
    install_failed: "Installation fehlgeschlagen: {0}",
    already_installing: "{0} wird bereits installiert",
    install_title: "Spiel installieren",
    install_description: "Der Installer des Spiels läuft still, danach sein Online-Fix.",
    game_installer: "Installer des Spiels",
    online_fix: "Online-Fix (optional)",
    not_chosen: "Nicht gewählt",
    installs_to: "Wird installiert nach {0}",
    cancel: "Abbrechen",
    remove: "Entfernen",
    games: |n| if n == 1 { "1 Spiel".into() } else { format!("{n} Spiele") },
};

pub static FR: Strings = Strings {
    library: "Bibliothèque",
    of: "{0} sur {1}",
    rescan: "Actualiser",
    rescan_help: "Analyser à nouveau les dossiers de la bibliothèque",
    add_folder: "Ajouter un dossier",
    add_game: "Ajouter un jeu",
    language: "Langue",
    search_placeholder: "Rechercher des jeux",
    search: "Recherche",
    close_search: "Fermer la recherche",
    play: "Jouer",
    stop: "Arrêter",
    running: "En cours",
    no_games: "Aucun jeu pour l\u{2019}instant",
    no_games_body: "Ajoutez un dossier contenant des jeux OnlineFix,\nou choisissez directement le .exe d\u{2019}un jeu.",
    no_matches: "Aucun résultat",
    no_matches_body: "Rien dans votre bibliothèque ne correspond à « {0} ».",
    steam_app: "Application Steam {0}",
    back: "Retour",
    back_help: "Retour à la bibliothèque (Échap)",
    folder: "Dossier",
    log: "Journal",
    copy_command: "Copier la commande",
    launch_options: "Options de lancement",
    defaults_hint: "Les champs vides utilisent les valeurs détectées.",
    no_proton: "Aucun Proton trouvé",
    executable: "Exécutable",
    no_executable: "Aucun exécutable trouvé",
    browse: "Parcourir",
    steam_app_id: "ID d\u{2019}application Steam",
    arguments: "Arguments",
    none: "aucun",
    dll_overrides: "Remplacements de DLL",
    environment: "Environnement",
    wine_prefix: "Préfixe Wine",
    no_steam: "Installation de Steam introuvable. Installez Steam et connectez-vous une fois.",
    stopped: "{0} arrêté",
    exited: "{0} s\u{2019}est fermé",
    exited_with: "{0} s\u{2019}est fermé : {1}",
    cant_save: "Impossible d\u{2019}enregistrer les réglages : {0}",
    already_running: "{0} est déjà lancé",
    starting_steam: "Steam n\u{2019}est pas lancé, démarrage en cours. Appuyez à nouveau sur Jouer une fois connecté.",
    cant_start_steam: "Impossible de démarrer Steam : {0}",
    launched: "{0} lancé avec {1}",
    launch_failed: "Échec du lancement : {0}",
    already_closing: "{0} est déjà en train de se fermer",
    stopping: "Arrêt de {0}",
    command_copied: "Commande de lancement copiée",
    add_folder_title: "Ajouter un dossier de jeux",
    folder_picker_failed: "Impossible d\u{2019}ouvrir le sélecteur de dossier : {0}",
    add_game_title: "Ajouter un jeu",
    choose_exe_title: "Choisir l\u{2019}exécutable du jeu",
    windows_programs: "Programmes Windows",
    no_log: "Pas encore de journal. Lancez d\u{2019}abord le jeu.",
    file_picker_failed: "Impossible d\u{2019}ouvrir le sélecteur de fichier : {0}",
    install: "Installer",
    install_game_title: "Choisir l\u{2019}installateur du jeu",
    install_fix_title: "Choisir le fix en ligne du jeu",
    installing: "Installation de {0}\u{2026}",
    installing_fix: "Installation du fix en ligne de {0}\u{2026}",
    installed: "{0} installé",
    install_failed: "Échec de l\u{2019}installation : {0}",
    already_installing: "{0} est déjà en cours d\u{2019}installation",
    install_title: "Installer un jeu",
    install_description: "L\u{2019}installateur du jeu s\u{2019}exécute en silence, puis son fix en ligne.",
    game_installer: "Installateur du jeu",
    online_fix: "Fix en ligne (facultatif)",
    not_chosen: "Aucun choisi",
    installs_to: "Sera installé dans {0}",
    cancel: "Annuler",
    remove: "Retirer",
    games: |n| if n <= 1 { format!("{n} jeu") } else { format!("{n} jeux") },
};

pub static ES: Strings = Strings {
    library: "Biblioteca",
    of: "{0} de {1}",
    rescan: "Actualizar",
    rescan_help: "Volver a escanear las carpetas de la biblioteca",
    add_folder: "Añadir carpeta",
    add_game: "Añadir juego",
    language: "Idioma",
    search_placeholder: "Buscar juegos",
    search: "Buscar",
    close_search: "Cerrar búsqueda",
    play: "Jugar",
    stop: "Detener",
    running: "En ejecución",
    no_games: "Aún no hay juegos",
    no_games_body: "Añade una carpeta con juegos de OnlineFix\no elige directamente el .exe de un juego.",
    no_matches: "Sin resultados",
    no_matches_body: "Nada en tu biblioteca coincide con «{0}».",
    steam_app: "App de Steam {0}",
    back: "Atrás",
    back_help: "Volver a la biblioteca (Esc)",
    folder: "Carpeta",
    log: "Registro",
    copy_command: "Copiar comando",
    launch_options: "Opciones de inicio",
    defaults_hint: "Los campos vacíos usan los valores detectados.",
    no_proton: "No se encontró Proton",
    executable: "Ejecutable",
    no_executable: "No se encontró ningún ejecutable",
    browse: "Examinar",
    steam_app_id: "ID de app de Steam",
    arguments: "Argumentos",
    none: "ninguno",
    dll_overrides: "Sustituciones de DLL",
    environment: "Entorno",
    wine_prefix: "Prefijo de Wine",
    no_steam: "No se encontró la instalación de Steam. Instala Steam e inicia sesión una vez.",
    stopped: "{0} detenido",
    exited: "{0} se cerró",
    exited_with: "{0} se cerró: {1}",
    cant_save: "No se pudieron guardar los ajustes: {0}",
    already_running: "{0} ya está en ejecución",
    starting_steam: "Steam no está abierto; iniciándolo. Pulsa Jugar de nuevo cuando hayas iniciado sesión.",
    cant_start_steam: "No se pudo iniciar Steam: {0}",
    launched: "{0} iniciado con {1}",
    launch_failed: "Error al iniciar: {0}",
    already_closing: "{0} ya se está cerrando",
    stopping: "Deteniendo {0}",
    command_copied: "Comando de inicio copiado",
    add_folder_title: "Añadir una carpeta de juegos",
    folder_picker_failed: "No se pudo abrir el selector de carpetas: {0}",
    add_game_title: "Añadir un juego",
    choose_exe_title: "Elige el ejecutable del juego",
    windows_programs: "Programas de Windows",
    no_log: "Aún no hay registro. Inicia el juego primero.",
    file_picker_failed: "No se pudo abrir el selector de archivos: {0}",
    install: "Instalar",
    install_game_title: "Elige el instalador del juego",
    install_fix_title: "Elige el fix online del juego",
    installing: "Instalando {0}\u{2026}",
    installing_fix: "Instalando el fix online de {0}\u{2026}",
    installed: "{0} instalado",
    install_failed: "Error al instalar: {0}",
    already_installing: "{0} ya se está instalando",
    install_title: "Instalar un juego",
    install_description: "El instalador del juego se ejecuta en silencio y luego su fix online.",
    game_installer: "Instalador del juego",
    online_fix: "Fix online (opcional)",
    not_chosen: "Sin elegir",
    installs_to: "Se instalará en {0}",
    cancel: "Cancelar",
    remove: "Quitar",
    games: |n| if n == 1 { "1 juego".into() } else { format!("{n} juegos") },
};

pub static PT_BR: Strings = Strings {
    library: "Biblioteca",
    of: "{0} de {1}",
    rescan: "Atualizar",
    rescan_help: "Escanear de novo as pastas da biblioteca",
    add_folder: "Adicionar pasta",
    add_game: "Adicionar jogo",
    language: "Idioma",
    search_placeholder: "Buscar jogos",
    search: "Buscar",
    close_search: "Fechar busca",
    play: "Jogar",
    stop: "Parar",
    running: "Em execução",
    no_games: "Nenhum jogo ainda",
    no_games_body: "Adicione uma pasta com jogos do OnlineFix\nou escolha o .exe de um jogo diretamente.",
    no_matches: "Nenhum resultado",
    no_matches_body: "Nada na sua biblioteca corresponde a \u{201c}{0}\u{201d}.",
    steam_app: "App da Steam {0}",
    back: "Voltar",
    back_help: "Voltar à biblioteca (Esc)",
    folder: "Pasta",
    log: "Log",
    copy_command: "Copiar comando",
    launch_options: "Opções de inicialização",
    defaults_hint: "Campos vazios usam os padrões detectados.",
    no_proton: "Nenhum Proton encontrado",
    executable: "Executável",
    no_executable: "Nenhum executável encontrado",
    browse: "Procurar",
    steam_app_id: "ID do app na Steam",
    arguments: "Argumentos",
    none: "nenhum",
    dll_overrides: "Substituições de DLL",
    environment: "Ambiente",
    wine_prefix: "Prefixo do Wine",
    no_steam: "Instalação da Steam não encontrada. Instale a Steam e entre uma vez.",
    stopped: "{0} parado",
    exited: "{0} fechou",
    exited_with: "{0} fechou: {1}",
    cant_save: "Não foi possível salvar as configurações: {0}",
    already_running: "{0} já está em execução",
    starting_steam: "A Steam não está aberta; iniciando. Pressione Jogar de novo depois de entrar.",
    cant_start_steam: "Não foi possível iniciar a Steam: {0}",
    launched: "{0} iniciado com {1}",
    launch_failed: "Falha ao iniciar: {0}",
    already_closing: "{0} já está fechando",
    stopping: "Parando {0}",
    command_copied: "Comando de inicialização copiado",
    add_folder_title: "Adicionar uma pasta de jogos",
    folder_picker_failed: "Não foi possível abrir o seletor de pastas: {0}",
    add_game_title: "Adicionar um jogo",
    choose_exe_title: "Escolha o executável do jogo",
    windows_programs: "Programas do Windows",
    no_log: "Ainda não há log. Inicie o jogo primeiro.",
    file_picker_failed: "Não foi possível abrir o seletor de arquivos: {0}",
    install: "Instalar",
    install_game_title: "Escolha o instalador do jogo",
    install_fix_title: "Escolha o fix online do jogo",
    installing: "Instalando {0}\u{2026}",
    installing_fix: "Instalando o fix online de {0}\u{2026}",
    installed: "{0} instalado",
    install_failed: "Falha na instalação: {0}",
    already_installing: "{0} já está sendo instalado",
    install_title: "Instalar um jogo",
    install_description: "O instalador do jogo roda em silêncio e depois o fix online.",
    game_installer: "Instalador do jogo",
    online_fix: "Fix online (opcional)",
    not_chosen: "Nenhum escolhido",
    installs_to: "Será instalado em {0}",
    cancel: "Cancelar",
    remove: "Remover",
    games: |n| if n == 1 { "1 jogo".into() } else { format!("{n} jogos") },
};

pub static PL: Strings = Strings {
    library: "Biblioteka",
    of: "{0} z {1}",
    rescan: "Skanuj",
    rescan_help: "Ponownie przeskanuj foldery biblioteki",
    add_folder: "Dodaj folder",
    add_game: "Dodaj grę",
    language: "Język",
    search_placeholder: "Szukaj gier",
    search: "Szukaj",
    close_search: "Zamknij wyszukiwanie",
    play: "Graj",
    stop: "Zatrzymaj",
    running: "Uruchomiona",
    no_games: "Nie ma jeszcze gier",
    no_games_body: "Dodaj folder z grami OnlineFix\nalbo wybierz bezpośrednio plik .exe gry.",
    no_matches: "Brak wyników",
    no_matches_body: "Nic w bibliotece nie pasuje do \u{201e}{0}\u{201d}.",
    steam_app: "Aplikacja Steam {0}",
    back: "Wstecz",
    back_help: "Wróć do biblioteki (Esc)",
    folder: "Folder",
    log: "Log",
    copy_command: "Kopiuj polecenie",
    launch_options: "Opcje uruchamiania",
    defaults_hint: "Puste pola używają wykrytych wartości domyślnych.",
    no_proton: "Nie znaleziono Protona",
    executable: "Plik wykonywalny",
    no_executable: "Nie znaleziono pliku wykonywalnego",
    browse: "Przeglądaj",
    steam_app_id: "ID aplikacji Steam",
    arguments: "Argumenty",
    none: "brak",
    dll_overrides: "Nadpisania DLL",
    environment: "Środowisko",
    wine_prefix: "Prefiks Wine",
    no_steam: "Nie znaleziono instalacji Steam. Zainstaluj Steam i zaloguj się raz.",
    stopped: "Zatrzymano {0}",
    exited: "Gra {0} została zamknięta",
    exited_with: "Gra {0} została zamknięta: {1}",
    cant_save: "Nie udało się zapisać ustawień: {0}",
    already_running: "Gra {0} jest już uruchomiona",
    starting_steam: "Steam nie działa, uruchamiam go. Naciśnij Graj ponownie po zalogowaniu.",
    cant_start_steam: "Nie udało się uruchomić Steam: {0}",
    launched: "Uruchomiono {0} z {1}",
    launch_failed: "Nie udało się uruchomić: {0}",
    already_closing: "Gra {0} już się zamyka",
    stopping: "Zatrzymywanie {0}",
    command_copied: "Skopiowano polecenie uruchomienia",
    add_folder_title: "Dodaj folder z grami",
    folder_picker_failed: "Nie udało się otworzyć wyboru folderu: {0}",
    add_game_title: "Dodaj grę",
    choose_exe_title: "Wybierz plik wykonywalny gry",
    windows_programs: "Programy Windows",
    no_log: "Nie ma jeszcze logu. Najpierw uruchom grę.",
    file_picker_failed: "Nie udało się otworzyć wyboru pliku: {0}",
    install: "Zainstaluj",
    install_game_title: "Wybierz instalator gry",
    install_fix_title: "Wybierz online fix gry",
    installing: "Instalowanie {0}\u{2026}",
    installing_fix: "Instalowanie online fixa do {0}\u{2026}",
    installed: "Zainstalowano {0}",
    install_failed: "Instalacja nie powiodła się: {0}",
    already_installing: "{0} już się instaluje",
    install_title: "Instalacja gry",
    install_description: "Instalator gry uruchomi się po cichu, a potem online fix.",
    game_installer: "Instalator gry",
    online_fix: "Online fix (opcjonalnie)",
    not_chosen: "Nie wybrano",
    installs_to: "Zostanie zainstalowana w {0}",
    cancel: "Anuluj",
    remove: "Usuń",
    games: polish,
};

pub static TR: Strings = Strings {
    library: "Kütüphane",
    of: "{0} / {1}",
    rescan: "Yeniden tara",
    rescan_help: "Kütüphane klasörlerini yeniden tara",
    add_folder: "Klasör ekle",
    add_game: "Oyun ekle",
    language: "Dil",
    search_placeholder: "Oyun ara",
    search: "Ara",
    close_search: "Aramayı kapat",
    play: "Oyna",
    stop: "Durdur",
    running: "Çalışıyor",
    no_games: "Henüz oyun yok",
    no_games_body: "OnlineFix oyunları içeren bir klasör ekleyin\nveya doğrudan bir oyunun .exe dosyasını seçin.",
    no_matches: "Eşleşme yok",
    no_matches_body: "Kütüphanenizde \u{201c}{0}\u{201d} ile eşleşen bir şey yok.",
    steam_app: "Steam uygulaması {0}",
    back: "Geri",
    back_help: "Kütüphaneye dön (Esc)",
    folder: "Klasör",
    log: "Günlük",
    copy_command: "Komutu kopyala",
    launch_options: "Başlatma seçenekleri",
    defaults_hint: "Boş alanlar algılanan varsayılanları kullanır.",
    no_proton: "Proton bulunamadı",
    executable: "Çalıştırılabilir dosya",
    no_executable: "Çalıştırılabilir dosya bulunamadı",
    browse: "Gözat",
    steam_app_id: "Steam uygulama kimliği",
    arguments: "Argümanlar",
    none: "yok",
    dll_overrides: "DLL geçersiz kılmaları",
    environment: "Ortam",
    wine_prefix: "Wine öneki",
    no_steam: "Steam kurulumu bulunamadı. Steam'i kurun ve bir kez giriş yapın.",
    stopped: "{0} durduruldu",
    exited: "{0} kapandı",
    exited_with: "{0} kapandı: {1}",
    cant_save: "Ayarlar kaydedilemedi: {0}",
    already_running: "{0} zaten çalışıyor",
    starting_steam: "Steam çalışmıyor, başlatılıyor. Giriş yaptıktan sonra yeniden Oyna'ya basın.",
    cant_start_steam: "Steam başlatılamadı: {0}",
    launched: "{0}, {1} ile başlatıldı",
    launch_failed: "Başlatma başarısız: {0}",
    already_closing: "{0} zaten kapanıyor",
    stopping: "{0} durduruluyor",
    command_copied: "Başlatma komutu kopyalandı",
    add_folder_title: "Oyun klasörü ekle",
    folder_picker_failed: "Klasör seçici açılamadı: {0}",
    add_game_title: "Oyun ekle",
    choose_exe_title: "Oyunun çalıştırılabilir dosyasını seçin",
    windows_programs: "Windows programları",
    no_log: "Henüz günlük yok. Önce oyunu başlatın.",
    file_picker_failed: "Dosya seçici açılamadı: {0}",
    install: "Yükle",
    install_game_title: "Oyunun yükleyicisini seç",
    install_fix_title: "Oyunun online fix\u{2019}ini seç",
    installing: "{0} yükleniyor\u{2026}",
    installing_fix: "{0} için online fix yükleniyor\u{2026}",
    installed: "{0} yüklendi",
    install_failed: "Yükleme başarısız: {0}",
    already_installing: "{0} zaten yükleniyor",
    install_title: "Oyun yükle",
    install_description: "Önce oyunun yükleyicisi sessizce çalışır, ardından online fix\u{2019}i.",
    game_installer: "Oyun yükleyicisi",
    online_fix: "Online fix (isteğe bağlı)",
    not_chosen: "Seçilmedi",
    installs_to: "{0} konumuna yüklenecek",
    cancel: "İptal",
    remove: "Kaldır",
    games: |n| format!("{n} oyun"),
};

pub static ZH_CN: Strings = Strings {
    library: "游戏库",
    of: "{0} / {1}",
    rescan: "重新扫描",
    rescan_help: "重新扫描游戏库文件夹",
    add_folder: "添加文件夹",
    add_game: "添加游戏",
    language: "语言",
    search_placeholder: "搜索游戏",
    search: "搜索",
    close_search: "关闭搜索",
    play: "开始游戏",
    stop: "停止",
    running: "运行中",
    no_games: "还没有游戏",
    no_games_body: "添加包含 OnlineFix 游戏的文件夹，\n或直接选择游戏的 .exe 文件。",
    no_matches: "没有匹配项",
    no_matches_body: "游戏库中没有与\u{201c}{0}\u{201d}匹配的内容。",
    steam_app: "Steam 应用 {0}",
    back: "返回",
    back_help: "返回游戏库 (Esc)",
    folder: "文件夹",
    log: "日志",
    copy_command: "复制命令",
    launch_options: "启动选项",
    defaults_hint: "留空的字段将使用检测到的默认值。",
    no_proton: "未找到 Proton",
    executable: "可执行文件",
    no_executable: "未找到可执行文件",
    browse: "浏览",
    steam_app_id: "Steam 应用 ID",
    arguments: "启动参数",
    none: "无",
    dll_overrides: "DLL 覆盖",
    environment: "环境变量",
    wine_prefix: "Wine 前缀",
    no_steam: "未找到 Steam 安装。请安装 Steam 并登录一次。",
    stopped: "{0} 已停止",
    exited: "{0} 已退出",
    exited_with: "{0} 已退出：{1}",
    cant_save: "无法保存设置：{0}",
    already_running: "{0} 已在运行",
    starting_steam: "Steam 未运行，正在启动。登录后请再次点击\u{201c}开始游戏\u{201d}。",
    cant_start_steam: "无法启动 Steam：{0}",
    launched: "已使用 {1} 启动 {0}",
    launch_failed: "启动失败：{0}",
    already_closing: "{0} 正在关闭",
    stopping: "正在停止 {0}",
    command_copied: "已复制启动命令",
    add_folder_title: "添加游戏文件夹",
    folder_picker_failed: "无法打开文件夹选择器：{0}",
    add_game_title: "添加游戏",
    choose_exe_title: "选择游戏的可执行文件",
    windows_programs: "Windows 程序",
    no_log: "还没有日志。请先启动游戏。",
    file_picker_failed: "无法打开文件选择器：{0}",
    install: "安装",
    install_game_title: "选择游戏的安装程序",
    install_fix_title: "选择游戏的联机补丁",
    installing: "正在安装 {0}\u{2026}",
    installing_fix: "正在为 {0} 安装联机补丁\u{2026}",
    installed: "已安装 {0}",
    install_failed: "安装失败：{0}",
    already_installing: "{0} 正在安装中",
    install_title: "安装游戏",
    install_description: "先静默运行游戏的安装程序，再安装联机补丁。",
    game_installer: "游戏安装程序",
    online_fix: "联机补丁（可选）",
    not_chosen: "未选择",
    installs_to: "将安装到 {0}",
    cancel: "取消",
    remove: "移除",
    games: |n| format!("{n} 个游戏"),
};

pub static JA: Strings = Strings {
    library: "ライブラリ",
    of: "{0} / {1}",
    rescan: "再スキャン",
    rescan_help: "ライブラリのフォルダーを再スキャン",
    add_folder: "フォルダーを追加",
    add_game: "ゲームを追加",
    language: "言語",
    search_placeholder: "ゲームを検索",
    search: "検索",
    close_search: "検索を閉じる",
    play: "プレイ",
    stop: "停止",
    running: "実行中",
    no_games: "まだゲームがありません",
    no_games_body: "OnlineFix のゲームが入ったフォルダーを追加するか、\nゲームの .exe を直接選んでください。",
    no_matches: "見つかりません",
    no_matches_body: "ライブラリに「{0}」に一致するものはありません。",
    steam_app: "Steam アプリ {0}",
    back: "戻る",
    back_help: "ライブラリに戻る (Esc)",
    folder: "フォルダー",
    log: "ログ",
    copy_command: "コマンドをコピー",
    launch_options: "起動オプション",
    defaults_hint: "空欄の項目には検出された既定値が使われます。",
    no_proton: "Proton が見つかりません",
    executable: "実行ファイル",
    no_executable: "実行ファイルが見つかりません",
    browse: "参照",
    steam_app_id: "Steam アプリ ID",
    arguments: "引数",
    none: "なし",
    dll_overrides: "DLL オーバーライド",
    environment: "環境変数",
    wine_prefix: "Wine プレフィックス",
    no_steam: "Steam のインストールが見つかりません。Steam をインストールして一度ログインしてください。",
    stopped: "{0} を停止しました",
    exited: "{0} が終了しました",
    exited_with: "{0} が終了しました: {1}",
    cant_save: "設定を保存できませんでした: {0}",
    already_running: "{0} はすでに実行中です",
    starting_steam: "Steam が起動していないため起動しています。ログインしたらもう一度プレイを押してください。",
    cant_start_steam: "Steam を起動できませんでした: {0}",
    launched: "{0} を {1} で起動しました",
    launch_failed: "起動に失敗しました: {0}",
    already_closing: "{0} はすでに終了処理中です",
    stopping: "{0} を停止しています",
    command_copied: "起動コマンドをコピーしました",
    add_folder_title: "ゲームのフォルダーを追加",
    folder_picker_failed: "フォルダー選択を開けませんでした: {0}",
    add_game_title: "ゲームを追加",
    choose_exe_title: "ゲームの実行ファイルを選択",
    windows_programs: "Windows プログラム",
    no_log: "ログはまだありません。先にゲームを起動してください。",
    file_picker_failed: "ファイル選択を開けませんでした: {0}",
    install: "インストール",
    install_game_title: "ゲームのインストーラーを選択",
    install_fix_title: "ゲームのオンライン修正を選択",
    installing: "{0} をインストールしています\u{2026}",
    installing_fix: "{0} のオンライン修正をインストールしています\u{2026}",
    installed: "{0} をインストールしました",
    install_failed: "インストールに失敗しました: {0}",
    already_installing: "{0} はすでにインストール中です",
    install_title: "ゲームをインストール",
    install_description: "ゲームのインストーラーを無音で実行し、続いてオンライン修正を適用します。",
    game_installer: "ゲームのインストーラー",
    online_fix: "オンライン修正（任意）",
    not_chosen: "未選択",
    installs_to: "インストール先: {0}",
    cancel: "キャンセル",
    remove: "削除",
    games: |n| format!("{n} 本のゲーム"),
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeSet, HashMap};

    fn placeholders(s: &str) -> BTreeSet<String> {
        (0..4).map(|i| format!("{{{i}}}")).filter(|p| s.contains(p.as_str())).collect()
    }

    /// A translation that drops or invents a `{0}` loses the game's name, or shows the braces.
    #[test]
    fn translations_keep_every_placeholder() {
        for lang in Lang::ALL {
            for ((field, en), (_, s)) in EN.all().into_iter().zip(lang.strings().all()) {
                assert!(!s.trim().is_empty(), "{lang:?}.{field} is empty");
                assert_eq!(placeholders(s), placeholders(en), "{lang:?}.{field}: {s:?}");
                assert_eq!(s.contains('\n'), en.contains('\n'), "{lang:?}.{field} breaks lines differently");
            }
        }
    }

    /// A string copied from English and never translated.
    #[test]
    fn languages_are_translated() {
        for lang in Lang::ALL.into_iter().filter(|&l| l != Lang::En) {
            let same: Vec<_> = EN
                .all()
                .into_iter()
                .zip(lang.strings().all())
                .filter(|((_, en), (_, s))| en == s)
                .map(|((field, _), _)| field)
                .collect();
            assert!(same.len() <= 3, "{lang:?} has these still in English: {same:?}");
        }
    }

    #[test]
    fn codes_and_names_round_trip() {
        for lang in Lang::ALL {
            assert_eq!(Lang::from_code(lang.code()), Some(lang));
            assert_eq!(Lang::from_name(lang.name()), Some(lang));
        }
        assert_eq!(Lang::from_code("xx"), None);
    }

    #[test]
    fn fill_puts_arguments_in_their_places() {
        assert_eq!(fill(ZH_CN.launched, &[&"Pong", &"Proton 9.0"]), "已使用 Proton 9.0 启动 Pong");
        assert_eq!(fill(EN.of, &[&3, &12]), "3 of 12");
        // A name that looks like a placeholder is left alone.
        assert_eq!(fill(EN.exited_with, &[&"{1}", &"boom"]), "{1} exited: boom");
    }

    #[test]
    fn plurals() {
        let ru: Vec<_> = [1, 2, 5, 11, 12, 21, 22, 25, 111, 0].map(RU.games).into();
        assert_eq!(
            ru,
            ["1 игра", "2 игры", "5 игр", "11 игр", "12 игр", "21 игра", "22 игры", "25 игр", "111 игр", "0 игр"]
        );
        let uk: Vec<_> = [1, 3, 14, 21].map(UK.games).into();
        assert_eq!(uk, ["1 гра", "3 гри", "14 ігор", "21 гра"]);
        let pl: Vec<_> = [0, 1, 2, 5, 12, 21, 22].map(PL.games).into();
        assert_eq!(pl, ["0 gier", "1 gra", "2 gry", "5 gier", "12 gier", "21 gier", "22 gry"]);
        assert_eq!([1, 2].map(EN.games), ["1 game", "2 games"]);
        assert_eq!([0, 1, 2].map(FR.games), ["0 jeu", "1 jeu", "2 jeux"]);
    }

    fn detect(vars: &[(&str, &str)]) -> Lang {
        let vars: HashMap<String, String> = vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        Lang::detect(|k| vars.get(k).cloned())
    }

    #[test]
    fn detects_the_system_language() {
        assert_eq!(detect(&[]), Lang::En);
        assert_eq!(detect(&[("LANG", "ru_RU.UTF-8")]), Lang::Ru);
        assert_eq!(detect(&[("LANG", "uk_UA.UTF-8")]), Lang::Uk);
        assert_eq!(detect(&[("LANG", "pt_PT.UTF-8")]), Lang::PtBr);
        assert_eq!(detect(&[("LANG", "zh_CN.UTF-8")]), Lang::ZhCn);
        assert_eq!(detect(&[("LANG", "zh_TW.UTF-8")]), Lang::En, "traditional Chinese");
        assert_eq!(detect(&[("LANG", "de_DE.UTF-8@euro")]), Lang::De);
        assert_eq!(detect(&[("LANG", "nl_NL.UTF-8")]), Lang::En, "a language we don't have");
        // LC_ALL wins over LC_MESSAGES, which wins over LANG.
        assert_eq!(detect(&[("LANG", "de_DE.UTF-8"), ("LC_MESSAGES", "ja_JP.UTF-8")]), Lang::Ja);
        assert_eq!(detect(&[("LC_ALL", "fr_FR.UTF-8"), ("LC_MESSAGES", "ja_JP.UTF-8")]), Lang::Fr);
        assert_eq!(detect(&[("LC_ALL", ""), ("LANG", "tr_TR.UTF-8")]), Lang::Tr, "an empty variable is unset");
        // LANGUAGE's list comes first, skipping what we don't have, but not under the C locale.
        assert_eq!(detect(&[("LANG", "de_DE.UTF-8"), ("LANGUAGE", "nl:pl:en")]), Lang::Pl);
        assert_eq!(detect(&[("LANG", "C.UTF-8"), ("LANGUAGE", "ru")]), Lang::En);
        assert_eq!(detect(&[("LC_ALL", "C"), ("LANG", "es_ES.UTF-8")]), Lang::En);
    }
}
