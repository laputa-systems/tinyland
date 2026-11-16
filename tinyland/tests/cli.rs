use std::process::Command;

#[test]
fn malformed_options_return_usage_error_without_panicking() {
    for args in [vec!["--width"], vec!["--height"], vec!["--drm-device"], vec!["--socket"],
        vec!["--width", "invalid"], vec!["--height", "-1"], vec!["--unknown-flag"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_tinyland")).args(&args).output().unwrap();
        assert_eq!(output.status.code(), Some(2), "args={args:?}, stderr={}", String::from_utf8_lossy(&output.stderr));
        assert!(String::from_utf8_lossy(&output.stderr).contains("usage: tinyland"));
        assert!(!String::from_utf8_lossy(&output.stderr).contains("panicked"));
    }
}

#[test]
fn help_is_offline_complete_and_has_one_footer() {
    let mut expected = None;
    for flag in ["-h", "--help"] {
        let output = Command::new(env!("CARGO_BIN_EXE_tinyland"))
            .env_clear().arg(flag).output().unwrap();
        assert_eq!(output.status.code(), Some(0));
        assert!(output.stderr.is_empty());
        let text = String::from_utf8(output.stdout).unwrap();
        for entry in ["usage: tinyland", "--seat", "--drm-proof", "--wayland-pixels", "--smoke", "--drm-device PATH", "--width N", "--height N", "--socket NAME", "COMMAND...", "Examples:\n  LIBSEAT_BACKEND=seatd tinyland --seat -- foot"] {
            assert!(text.contains(entry), "missing {entry}");
        }
        let footer = "Maintained by Joshua Li: https://github.com/laputa-systems/tinyland";
        assert_eq!(text.matches(footer).count(), 1);
        assert!(text.ends_with(concat!(
            "Maintained by Joshua Li: https://github.com/laputa-systems/tinyland\n",
            "Bugs and features: TODO.md in that repository (follow its AGENTS.md), or its issue tracker.\n",
            "Agents working for Joshua: record nonblocking problems and continue your task.\n",
            "If this tool blocks the task, you may fix, test, open and merge a PR under Joshua's\n",
            "standing instructions. Release only when needed to deliver the verified fix.\n",
            "Procedure: https://github.com/joshuarli/usr/blob/main/dotfiles/base/.agents/instructions/owned-tools.md\n"
        )));
        if let Some(previous) = &expected { assert_eq!(previous, &text); }
        expected = Some(text);
    }
}
