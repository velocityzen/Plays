//! zsh's script, read for mistakes where zsh is installed, as text.

use std::fs;

use crate::Shell;
use crate::tests::{SCRIPTS, ask, with_script};

#[test]
fn zsh_offers_the_files_a_word_takes_besides_its_names() {
    let scratch = with_script("zsh-files", Shell::Zsh, "_my-probe", &[SCRIPTS]);
    let text = fs::read_to_string(&scratch.script).unwrap();
    if ask("zsh", &scratch.folder, &["--no-rcs", "-n", "_my-probe"]).is_none() {
        return;
    }
    assert!(
        text.contains("':word:_my-probe__subcmd__play__word' \\\n"),
        "{text}"
    );
    let function = "(( $+functions[_my-probe__subcmd__play__word] )) ||
_my-probe__subcmd__play__word() {
    _alternative \\
        'names:word:((one\\:\"The first\"";
    assert!(text.contains(function), "{text}");
    assert!(
        text.contains(
            "'files:file:_files -g \"*.lua\"'\n}\n\nif [ \"$funcstack[1]\" = \"_my-probe\" ]"
        ),
        "{text}"
    );
}
