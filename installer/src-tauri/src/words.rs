//! The installer's languages. The installer isn't on the family's base, so it picks the language here, once, the way
//! shared/web/i18n.js does for every program (the system's language; Traditional Chinese for Taiwan, Hong Kong and Macau;
//! every Spanish takes es-419 and every Portuguese pt-BR; English when none fits), and hands the page its table.
//! The few words said before there is a window (no WebView2, a damaged installer) and the file-type name written into
//! the registry are here, in Rust; everything the page says is in installer/src/lang/<code>.json.

/// The languages, as shared/web/i18n.js LANGS has them.
pub const CODES: [&str; 9] = ["zh", "zh-Hant", "en", "ja", "ko", "es-419", "pt-BR", "fr", "de"];

/// The page's tables, built into the program (each a few KB): (code, JSON).
pub const TABLES: [(&str, &str); 8] = [
    ("en", include_str!("../../src/lang/en.json")),
    ("zh-Hant", include_str!("../../src/lang/zh-Hant.json")),
    ("ja", include_str!("../../src/lang/ja.json")),
    ("ko", include_str!("../../src/lang/ko.json")),
    ("es-419", include_str!("../../src/lang/es-419.json")),
    ("pt-BR", include_str!("../../src/lang/pt-BR.json")),
    ("fr", include_str!("../../src/lang/fr.json")),
    ("de", include_str!("../../src/lang/de.json")),
];

/// A table's JSON (English for Chinese, which needs none, and as the fallback under every other).
pub fn table(code: &str) -> &'static str {
    TABLES.iter().find(|(c, _)| *c == code).map(|(_, t)| *t).unwrap_or(TABLES[0].1)
}

/// The language from a Windows LANGID (primary language in the low 10 bits, sublanguage above).
pub fn from_langid(id: u16) -> &'static str {
    let (primary, sub) = (id & 0x3ff, id >> 10);
    match primary {
        // Chinese: Taiwan 1, Hong Kong 3, Macau 5 write Traditional; PRC 2 and Singapore 4 Simplified
        0x04 => {
            if matches!(sub, 1 | 3 | 5) {
                "zh-Hant"
            } else {
                "zh"
            }
        }
        0x11 => "ja",
        0x12 => "ko",
        0x0a => "es-419",
        0x16 => "pt-BR",
        0x0c => "fr",
        0x07 => "de",
        _ => "en",
    }
}

/// The language this run speaks: DOTANY_SETUP_LANG (the tests; any of CODES), else the system's.
pub fn system_lang() -> &'static str {
    if let Ok(l) = std::env::var("DOTANY_SETUP_LANG") {
        if let Some(c) = CODES.iter().find(|c| **c == l) {
            return c;
        }
    }
    extern "system" {
        fn GetUserDefaultUILanguage() -> u16;
    }
    from_langid(unsafe { GetUserDefaultUILanguage() })
}

/// What a message box says, by key.
pub enum Say {
    /// no WebView2: the text, the download button, the close button
    NoWebView2,
    /// the uninstaller without WebView2: the question, uninstall, cancel
    UninstallWithoutWindow,
    Uninstalled,
    /// {} is the open programs, joined with list_sep
    StillOpen,
    CouldntUninstall,
    Damaged,
}

/// The words for a message box: (text, first button, second button); a single message has no buttons.
pub fn say(lang: &str, what: Say) -> (&'static str, &'static str, &'static str) {
    use Say::*;
    match (lang, what) {
        ("zh", NoWebView2) => (
            "这台电脑缺少 Microsoft Edge WebView2 运行时。DotAny 的窗口都靠它显示，请先装好它，再运行 DotAny 安装程序。",
            "下载 WebView2",
            "关闭",
        ),
        ("zh", UninstallWithoutWindow) => (
            "这台电脑缺少 Microsoft Edge WebView2 运行时，卸载窗口打不开。要直接卸载 DotAny 吗？你的设置会保留。",
            "卸载",
            "取消",
        ),
        ("zh", Uninstalled) => ("DotAny 已卸载。", "", ""),
        ("zh", StillOpen) => ("{} 还开着，关掉后再卸载。", "", ""),
        ("zh", CouldntUninstall) => ("没能卸载 DotAny，请稍后再试。", "", ""),
        ("zh", Damaged) => ("安装程序已损坏，请重新下载。", "", ""),

        ("zh-Hant", NoWebView2) => (
            "這台電腦缺少 Microsoft Edge WebView2 執行階段。DotAny 的視窗都靠它顯示，請先安裝它，再執行 DotAny 安裝程式。",
            "下載 WebView2",
            "關閉",
        ),
        ("zh-Hant", UninstallWithoutWindow) => (
            "這台電腦缺少 Microsoft Edge WebView2 執行階段，無法開啟解除安裝視窗。要直接解除安裝 DotAny 嗎？設定會保留。",
            "解除安裝",
            "取消",
        ),
        ("zh-Hant", Uninstalled) => ("DotAny 已解除安裝。", "", ""),
        ("zh-Hant", StillOpen) => ("{} 仍在執行，請關閉後再解除安裝。", "", ""),
        ("zh-Hant", CouldntUninstall) => ("無法解除安裝 DotAny，請稍後再試。", "", ""),
        ("zh-Hant", Damaged) => ("安裝程式已損毀，請重新下載。", "", ""),

        ("ja", NoWebView2) => (
            "このコンピューターには Microsoft Edge WebView2 ランタイムがありません。DotAny のウィンドウはこれを使って表示されます。先にインストールしてから、DotAny のセットアップをもう一度実行してください。",
            "WebView2 をダウンロード",
            "閉じる",
        ),
        ("ja", UninstallWithoutWindow) => (
            "このコンピューターには Microsoft Edge WebView2 ランタイムがないため、アンインストールの画面を開けません。このまま DotAny をアンインストールしますか？設定は残ります。",
            "アンインストール",
            "キャンセル",
        ),
        ("ja", Uninstalled) => ("DotAny をアンインストールしました。", "", ""),
        ("ja", StillOpen) => ("{} が開いています。閉じてからアンインストールしてください。", "", ""),
        ("ja", CouldntUninstall) => {
            ("DotAny をアンインストールできませんでした。しばらくしてからもう一度お試しください。", "", "")
        }
        ("ja", Damaged) => ("インストーラーが破損しています。もう一度ダウンロードしてください。", "", ""),

        ("ko", NoWebView2) => (
            "이 컴퓨터에 Microsoft Edge WebView2 런타임이 없습니다. DotAny의 모든 창은 이 런타임으로 표시되므로, 먼저 설치한 다음 DotAny 설치 프로그램을 다시 실행하세요.",
            "WebView2 다운로드",
            "닫기",
        ),
        ("ko", UninstallWithoutWindow) => (
            "이 컴퓨터에 Microsoft Edge WebView2 런타임이 없어 제거 창을 열 수 없습니다. 그래도 DotAny를 제거하시겠습니까? 설정은 유지됩니다.",
            "제거",
            "취소",
        ),
        ("ko", Uninstalled) => ("DotAny를 제거했습니다.", "", ""),
        ("ko", StillOpen) => ("다음 앱이 아직 열려 있습니다: {}. 닫은 다음 다시 제거하세요.", "", ""),
        ("ko", CouldntUninstall) => ("DotAny를 제거하지 못했습니다. 잠시 후 다시 시도하세요.", "", ""),
        ("ko", Damaged) => ("설치 프로그램이 손상되었습니다. 다시 다운로드하세요.", "", ""),

        ("es-419", NoWebView2) => (
            "Falta Microsoft Edge WebView2 Runtime en este equipo. Todas las ventanas de DotAny se muestran con él: hay que instalarlo primero y luego volver a ejecutar el instalador de DotAny.",
            "Descargar WebView2",
            "Cerrar",
        ),
        ("es-419", UninstallWithoutWindow) => (
            "Falta Microsoft Edge WebView2 Runtime en este equipo, así que no se puede abrir la ventana de desinstalación. ¿Desinstalar DotAny de todos modos? Se conserva la configuración.",
            "Desinstalar",
            "Cancelar",
        ),
        ("es-419", Uninstalled) => ("DotAny se desinstaló.", "", ""),
        ("es-419", StillOpen) => ("{} sigue abierto. Hay que cerrarlo y volver a desinstalar.", "", ""),
        ("es-419", CouldntUninstall) => ("No se pudo desinstalar DotAny. Volver a intentarlo más tarde.", "", ""),
        ("es-419", Damaged) => ("El instalador está dañado. Hay que descargarlo de nuevo.", "", ""),

        ("pt-BR", NoWebView2) => (
            "Este computador não tem o Microsoft Edge WebView2 Runtime. Todas as janelas do DotAny dependem dele: é preciso instalá-lo primeiro e depois executar de novo o instalador do DotAny.",
            "Baixar o WebView2",
            "Fechar",
        ),
        ("pt-BR", UninstallWithoutWindow) => (
            "Este computador não tem o Microsoft Edge WebView2 Runtime, então a janela de desinstalação não pode ser aberta. Desinstalar o DotAny mesmo assim? As configurações são mantidas.",
            "Desinstalar",
            "Cancelar",
        ),
        ("pt-BR", Uninstalled) => ("O DotAny foi desinstalado.", "", ""),
        ("pt-BR", StillOpen) => ("{} ainda está aberto. É preciso fechá-lo e desinstalar de novo.", "", ""),
        ("pt-BR", CouldntUninstall) => ("Não foi possível desinstalar o DotAny. Tentar novamente mais tarde.", "", ""),
        ("pt-BR", Damaged) => ("O instalador está danificado. É preciso baixá-lo de novo.", "", ""),

        ("fr", NoWebView2) => (
            "Microsoft Edge WebView2 Runtime est absent de cet ordinateur. Toutes les fenêtres de DotAny en ont besoin : l’installer d’abord, puis relancer le programme d’installation de DotAny.",
            "Télécharger WebView2",
            "Fermer",
        ),
        ("fr", UninstallWithoutWindow) => (
            "Microsoft Edge WebView2 Runtime est absent de cet ordinateur : la fenêtre de désinstallation ne peut pas s’ouvrir. Désinstaller DotAny quand même ? Les paramètres sont conservés.",
            "Désinstaller",
            "Annuler",
        ),
        ("fr", Uninstalled) => ("DotAny a été désinstallé.", "", ""),
        ("fr", StillOpen) => ("{} est encore ouvert. Le fermer, puis relancer la désinstallation.", "", ""),
        ("fr", CouldntUninstall) => ("Impossible de désinstaller DotAny. Réessayer plus tard.", "", ""),
        ("fr", Damaged) => ("Le programme d’installation est endommagé. Le télécharger à nouveau.", "", ""),

        ("de", NoWebView2) => (
            "Auf diesem Computer fehlt die Microsoft Edge WebView2-Runtime. Alle DotAny-Fenster werden damit angezeigt. Bitte zuerst die Runtime installieren und dann das DotAny-Setup erneut ausführen.",
            "WebView2 herunterladen",
            "Schließen",
        ),
        ("de", UninstallWithoutWindow) => (
            "Auf diesem Computer fehlt die Microsoft Edge WebView2-Runtime, daher kann das Deinstallationsfenster nicht geöffnet werden. DotAny trotzdem deinstallieren? Die Einstellungen bleiben erhalten.",
            "Deinstallieren",
            "Abbrechen",
        ),
        ("de", Uninstalled) => ("DotAny wurde deinstalliert.", "", ""),
        ("de", StillOpen) => ("{} ist noch geöffnet. Bitte schließen und dann erneut deinstallieren.", "", ""),
        ("de", CouldntUninstall) => {
            ("DotAny konnte nicht deinstalliert werden. Bitte später erneut versuchen.", "", "")
        }
        ("de", Damaged) => ("Das Installationsprogramm ist beschädigt. Bitte erneut herunterladen.", "", ""),

        (_, NoWebView2) => (
            "This computer doesn't have the Microsoft Edge WebView2 Runtime. Every DotAny window is drawn with it, so please install it first, then run DotAny Setup again.",
            "Download WebView2",
            "Close",
        ),
        (_, UninstallWithoutWindow) => (
            "This computer doesn't have the Microsoft Edge WebView2 Runtime, so the uninstall window can't open. Uninstall DotAny anyway? Your settings are kept.",
            "Uninstall",
            "Cancel",
        ),
        (_, Uninstalled) => ("DotAny has been uninstalled.", "", ""),
        (_, StillOpen) => ("{} is still open. Close it, then uninstall again.", "", ""),
        (_, CouldntUninstall) => ("DotAny couldn't be uninstalled. Please try again later.", "", ""),
        (_, Damaged) => ("The installer is damaged. Please download it again.", "", ""),
    }
}

/// How a list of names is joined: 、 in Chinese and Japanese, a comma elsewhere.
pub fn list_sep(lang: &str) -> &'static str {
    if matches!(lang, "zh" | "zh-Hant" | "ja") { "、" } else { ", " }
}

/// The file-type name Windows shows for a format DotAny opens (File Explorer's Type column): TXT 文件, TXT file.
pub fn file_kind(lang: &str, ext: &str) -> String {
    let e = ext.to_uppercase();
    match lang {
        "zh" => format!("{e} 文件"),
        "zh-Hant" => format!("{e} 檔案"),
        "ja" => format!("{e} ファイル"),
        "ko" => format!("{e} 파일"),
        "es-419" => format!("Archivo {e}"),
        "pt-BR" => format!("Arquivo {e}"),
        "fr" => format!("Fichier {e}"),
        "de" => format!("{e}-Datei"),
        _ => format!("{e} file"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn langids() {
        assert_eq!(from_langid(0x0804), "zh"); // zh-CN
        assert_eq!(from_langid(0x0404), "zh-Hant"); // zh-TW
        assert_eq!(from_langid(0x0c04), "zh-Hant"); // zh-HK
        assert_eq!(from_langid(0x1404), "zh-Hant"); // zh-MO
        assert_eq!(from_langid(0x1004), "zh"); // zh-SG
        assert_eq!(from_langid(0x0c0a), "es-419"); // es-ES
        assert_eq!(from_langid(0x080a), "es-419"); // es-MX
        assert_eq!(from_langid(0x0816), "pt-BR"); // pt-PT
        assert_eq!(from_langid(0x0411), "ja");
        assert_eq!(from_langid(0x0412), "ko");
        assert_eq!(from_langid(0x040c), "fr");
        assert_eq!(from_langid(0x0407), "de");
        assert_eq!(from_langid(0x0419), "en"); // ru: no table, English
    }

    #[test]
    fn every_language_has_its_words() {
        for c in CODES {
            for s in [
                Say::NoWebView2,
                Say::UninstallWithoutWindow,
                Say::Uninstalled,
                Say::StillOpen,
                Say::CouldntUninstall,
                Say::Damaged,
            ] {
                let (text, ..) = say(c, s);
                assert!(!text.is_empty());
            }
            assert!(say(c, Say::StillOpen).0.contains("{}"));
            assert!(c == "zh" || c == "en" || table(c) != table("en"), "{c} has its table");
        }
    }
}
