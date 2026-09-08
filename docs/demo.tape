# Recording for docs/images/demo.gif — the README's hero.
#
#   vhs docs/demo.tape
#
# Run it from the repository root with a release build already present:
#
#   cargo build --release -p agentos-cli
#
# HOME is redirected at a scratch directory so the frames show a demonstration
# sandbox rather than whoever happened to record it, and `--scripted` replays a
# fixed model transcript so the run is the same every time. The permission
# decisions in the recording are real ones taken by the real engine; only the
# model's side of the conversation is canned.

Output docs/images/demo.gif

Set Shell bash
Set FontSize 13
Set Width 1300
Set Height 860
Set Padding 25
Set Theme "Catppuccin Mocha"
Set TypingSpeed 45ms

Hide
Type "export HOME=/tmp/agentos-demo PATH=$PWD/target/release:$PATH"
Enter
Type "rm -rf /tmp/agentos-demo && mkdir -p /tmp/agentos-demo"
Enter
Type "export PS1='$ '"
Enter
Type "clear"
Enter
Show

Sleep 1s
Type "agentos demo --scripted"
Sleep 700ms
Enter
Sleep 6s
