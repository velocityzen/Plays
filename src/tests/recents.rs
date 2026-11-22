use gpui::TestAppContext;

use super::*;

#[gpui::test]
fn it_keeps_the_latest_few_once_each(cx: &mut TestAppContext) {
    cx.update(|cx| {
        for n in 0..10 {
            COMMANDS.note(&format!("test.c{n}"), cx);
        }
        let kept: Vec<String> = (2..10).rev().map(|n| format!("test.c{n}")).collect();
        assert_eq!(COMMANDS.list(cx), kept);
        assert_eq!(kept.len(), COMMANDS.kept);
        COMMANDS.note("test.c5", cx);
        let listed = COMMANDS.list(cx);
        assert_eq!(listed[0], "test.c5");
        assert_eq!(
            listed.iter().filter(|id| *id == "test.c5").count(),
            1,
            "once"
        );
        assert_eq!(listed.len(), COMMANDS.kept);
    });
}

#[gpui::test]
fn a_file_is_noted_by_its_full_path(cx: &mut TestAppContext) {
    cx.update(|cx| {
        note_file(Path::new("clip.mp4"), cx);
        let full = std::env::current_dir().unwrap().join("clip.mp4");
        assert_eq!(FILES.list(cx), [full.to_string_lossy()]);
    });
}

#[gpui::test]
fn each_list_is_kept_apart_in_its_own_file_and_cleared_alone(cx: &mut TestAppContext) {
    let folder = Folder::for_test("recents-apart");
    cx.update(|cx| {
        cx.set_global(folder.clone());
        COMMANDS.note("app.about", cx);
        FILES.note("/media/a.mkv", cx);
        FILES.note("/media/b.mkv", cx);
        assert_eq!(COMMANDS.list(cx), ["app.about"]);
        assert_eq!(FILES.list(cx), ["/media/b.mkv", "/media/a.mkv"]);

        forget(cx);
        assert_eq!(
            FILES.list(cx),
            ["/media/b.mkv", "/media/a.mkv"],
            "read again"
        );
        FILES.clear(cx);
        assert!(FILES.list(cx).is_empty());
        assert_eq!(COMMANDS.list(cx), ["app.about"], "the other kept");
        forget(cx);
        assert!(FILES.list(cx).is_empty(), "cleared in its file too");
    });
}
