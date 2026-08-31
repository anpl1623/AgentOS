See [CHANGELOG.md](https://github.com/anpl1623/AgentOS/blob/main/CHANGELOG.md) for what is in this
release, and [SECURITY.md](https://github.com/anpl1623/AgentOS/blob/main/SECURITY.md) for what
AgentOS does and does not defend against. Read the second one before pointing an agent at anything
you care about.

## What to download

| You want | Take |
| --- | --- |
| The desktop application, macOS | `AgentOS_*_universal.dmg` |
| The desktop application, Windows | `AgentOS_*_x64-setup.exe` or the `.msi` |
| The desktop application, Linux | `agent-os_*_amd64.deb` or the `.AppImage` |
| Just the `agentos` CLI | `agentos-*-<your-target>.tar.gz`, or `.zip` on Windows |

Every CLI archive ships a `.sha256` beside it. Verify before running:

```bash
sha256sum -c agentos-<version>-<target>.tar.gz.sha256
```

## These builds are not code-signed

AgentOS has no Apple Developer certificate and no Windows code-signing certificate, so the installers
are unsigned. That is a real cost and it is stated plainly rather than buried:

- **macOS** will refuse to open the app on first launch. Right-click the application and choose Open,
  or clear the quarantine attribute yourself:

  ```bash
  xattr -dr com.apple.quarantine /Applications/AgentOS.app
  ```

- **Windows** SmartScreen will warn that the publisher is unknown. More Info → Run anyway.

Neither step is something you should perform on software you have not decided to trust. Building from
source is supported and takes one command — see the README.
