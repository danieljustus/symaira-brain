//! Windows-shaped config values: an unescaped path inside a basic string.
//!
//! `BurntSushi` fails on `"C:\Users\me"` (`\U` needs eight hex digits) and on
//! `"D:\a\_temp"` (`\a`, `\_` are no escape), so `config.Load` errors and
//! `loadSkillsEnv` replaces the whole config with the defaults. These cases run
//! on every host: the decision does not depend on the platform, only the input.

use symbrain_skills::config_toml::{ConfigFile, LoadOutcome, NeedsGo, SkillsConfig, load};

fn defaults() -> SkillsConfig {
    SkillsConfig {
        library_dir: "/d/library".to_owned(),
        render_dir: "/d/render".to_owned(),
        cache_dir: "/d/cache".to_owned(),
        profiles_dir: "/d/profiles".to_owned(),
        base_dir: "/d/base".to_owned(),
        vcs_enabled: true,
    }
}

fn file(content: &[u8]) -> Option<ConfigFile<'_>> {
    Some(ConfigFile {
        path: "config.toml",
        content,
    })
}

fn global(text: &str) -> LoadOutcome {
    load(&defaults(), file(text.as_bytes()), None)
}

#[test]
fn invalid_escapes_in_single_line_basic_strings_are_rejected() {
    for text in [
        "library_dir = \"C:\\Users\\me\\library\"\n",
        "library_dir = \"D:\\a\\_temp\\x\"\n",
        "library_dir = \"x\\ y\"\n",
        "library_dir = \"x\\\ty\"\n",
        "library_dir = \"\\xZZ\"\n",
        "library_dir = \"\\u12\"\n",
        "library_dir = \"\\U0000004\"\n",
        "library_dir = \"\\\u{e9}\"\n",
        "# \"quoted\" comment\nrender_dir = 'lit'\n\"k\\q\" = 1\n",
        "cache_dir = \"fine\\t\"\nlibrary_dir = \"\\q\"\n",
        "\u{feff}library_dir = \"C:\\Users\"\n",
        // The bad escape comes before the string ends, whatever follows.
        "library_dir = \"unterminated \\q\n",
        "library_dir = \"\\q",
    ] {
        assert_eq!(global(text), LoadOutcome::Rejected, "{text:?}");
    }
}

#[test]
fn rejection_follows_the_global_then_project_merge() {
    let valid = b"library_dir = \"/x/library\"\n";
    let windows = b"library_dir = \"C:\\Users\\me\"\n";
    assert_eq!(
        load(&defaults(), file(windows), file(valid)),
        LoadOutcome::Rejected
    );
    assert_eq!(
        load(&defaults(), file(valid), file(windows)),
        LoadOutcome::Rejected
    );
    // An apply error in the global file ends the load before the project file
    // is parsed, so the unreadable project file never matters.
    assert!(matches!(
        load(&defaults(), file(b"cache_dir = true\n"), file(windows)),
        LoadOutcome::Error(_)
    ));
}

#[test]
fn unmodelled_or_valid_shapes_keep_their_outcome() {
    for text in [
        // Multi-line strings and unterminated strings stay with Go.
        "library_dir = \"\"\"C:\\Users\"\"\"\n",
        "library_dir = '''C:\\Users'''\nrender_dir = \"\\q\"\n",
        "library_dir = \"unterminated\n",
        "library_dir = \"unterminated",
        "library_dir = \"trailing backslash \\",
        "library_dir = \n",
    ] {
        assert_eq!(
            global(text),
            LoadOutcome::NeedsGo(NeedsGo::Syntax),
            "{text:?}"
        );
    }
    // Literal strings and valid escapes decode natively.
    let literal = global("library_dir = 'C:\\Users\\me'\n");
    assert!(
        matches!(&literal, LoadOutcome::Loaded(config) if config.library_dir == "C:\\Users\\me"),
        "{literal:?}"
    );
    let escaped = global("library_dir = \"C:\\\\Users\\x41\\u0042\\U00000043\"\n");
    assert!(
        matches!(&escaped, LoadOutcome::Loaded(config) if config.library_dir == "C:\\UsersABC"),
        "{escaped:?}"
    );
    let comment = global("# \"C:\\Users\"\nlibrary_dir = \"/ok\"\n");
    assert!(matches!(comment, LoadOutcome::Loaded(_)), "{comment:?}");
}
