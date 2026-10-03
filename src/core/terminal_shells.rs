//! The shells the terminal pane can run, found on this PC: Command Prompt,
//! Windows PowerShell, PowerShell 7 (winget/MSI/Store installs), Git Bash,
//! every installed WSL distribution (Ubuntu, ...), and MSYS2/Cygwin bash.
//! Only shells that are actually installed are listed. Programs are always
//! started from their full path (see `system_paths`).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShellKind {
    Cmd,
    WindowsPowerShell,
    PowerShell7,
    GitBash,
    /// A WSL distribution, by name.
    Wsl(String),
    Msys2,
    Cygwin,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellProfile {
    /// Stable id saved in settings: "cmd", "powershell", "pwsh",
    /// "git-bash", "wsl:Ubuntu", "msys2", "cygwin".
    pub id: String,
    pub name: String,
    pub kind: ShellKind,
    pub program: PathBuf,
}

/// How to start a shell in a folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launch {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    /// `args` are already quoted for the command line (cmd's own quoting
    /// rules, which the usual escaping would break).
    pub raw_args: bool,
}

impl ShellProfile {
    pub fn launch(&self, dir: &Path) -> Launch {
        let mut args = Vec::new();
        let mut env = Vec::new();
        let mut raw_args = false;
        match &self.kind {
            // Clink loads itself through cmd's AutoRun when its autorun is
            // installed; when it isn't, inject it here so the pane gets it too.
            ShellKind::Cmd => {
                if let Some(clink) = clink_to_inject() {
                    args = clink_args(clink);
                    raw_args = true;
                }
            }
            ShellKind::WindowsPowerShell | ShellKind::PowerShell7 => args.push("-NoLogo".into()),
            ShellKind::GitBash | ShellKind::Msys2 | ShellKind::Cygwin => {
                args.extend(["--login".to_string(), "-i".to_string()]);
                // Keeps the login profile from changing to the home folder.
                env.push(("CHERE_INVOKING".into(), "1".into()));
            }
            ShellKind::Wsl(distro) => {
                args.extend(["-d".to_string(), distro.clone(), "--cd".to_string(), dir.display().to_string()]);
            }
        }
        Launch { program: self.program.clone(), args, env, raw_args }
    }

    /// Text that clears the prompt's current line and changes to `dir`
    /// (for "Go To Current Folder"), ending with Enter.
    pub fn cd_command(&self, dir: &Path) -> String {
        let path = dir.display().to_string();
        let single = |s: &str, escape: &str| s.replace('\'', escape);
        match &self.kind {
            // Esc clears the line in cmd and PowerShell.
            ShellKind::Cmd => format!("\x1bcd /d \"{path}\"\r"),
            ShellKind::WindowsPowerShell | ShellKind::PowerShell7 => {
                format!("\x1bSet-Location -LiteralPath '{}'\r", single(&path, "''"))
            }
            // Ctrl+U clears the line in bash.
            ShellKind::GitBash | ShellKind::Msys2 | ShellKind::Cygwin => {
                format!("\x15cd \"$(cygpath -u '{}')\"\r", single(&path, "'\\''"))
            }
            ShellKind::Wsl(_) => format!("\x15cd \"$(wslpath -u '{}')\"\r", single(&path, "'\\''")),
        }
    }

    /// Phosphor glyph for the shell's tab and menu entry.
    pub fn icon(&self) -> &'static str {
        use egui_phosphor::regular;
        match self.kind {
            ShellKind::Cmd => regular::TERMINAL,
            ShellKind::WindowsPowerShell | ShellKind::PowerShell7 => regular::WINDOWS_LOGO,
            ShellKind::GitBash => regular::GIT_BRANCH,
            ShellKind::Wsl(_) => regular::LINUX_LOGO,
            ShellKind::Msys2 | ShellKind::Cygwin => regular::CURRENCY_DOLLAR,
        }
    }
}

/// cmd arguments that start Clink: `/s /k ""<clink.bat>" inject"`. With
/// `/s`, cmd strips just the outer quotes, so a path with spaces and
/// parentheses (`C:\Program Files (x86)\clink`) survives.
fn clink_args(clink_bat: &Path) -> Vec<String> {
    vec!["/s".into(), "/k".into(), format!("\"\"{}\" inject\"", clink_bat.display())]
}

/// Clink's `clink.bat`, if Clink is installed but doesn't already load
/// through cmd's AutoRun (checked once).
fn clink_to_inject() -> Option<&'static Path> {
    static FOUND: OnceLock<Option<PathBuf>> = OnceLock::new();
    FOUND
        .get_or_init(|| {
            let autorun_has_clink = cmd_autorun().iter().any(|v| v.to_lowercase().contains("clink"));
            if autorun_has_clink { None } else { find_clink() }
        })
        .as_deref()
}

/// cmd's AutoRun commands (per user and for the machine).
fn cmd_autorun() -> Vec<String> {
    use windows::Win32::System::Registry::*;
    use windows::core::HSTRING;
    let mut out = Vec::new();
    for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        let mut buf = vec![0u16; 2048];
        let mut len = (buf.len() * 2) as u32;
        let status = unsafe {
            RegGetValueW(
                root,
                &HSTRING::from(r"Software\Microsoft\Command Processor"),
                &HSTRING::from("AutoRun"),
                RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | RRF_NOEXPAND,
                None,
                Some(buf.as_mut_ptr().cast()),
                Some(&mut len),
            )
        };
        if status.is_ok() {
            let chars = (len as usize / 2).saturating_sub(1).min(buf.len());
            out.push(String::from_utf16_lossy(&buf[..chars]));
        }
    }
    out
}

/// Clink's install folder: the installer/winget default, Scoop, or PATH.
fn find_clink() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    for var in ["ProgramFiles(x86)", "ProgramW6432", "ProgramFiles"] {
        if let Some(dir) = env_dir(var) {
            candidates.push(dir.join(r"clink\clink.bat"));
        }
    }
    if let Some(local) = env_dir("LOCALAPPDATA") {
        candidates.push(local.join(r"Programs\clink\clink.bat"));
    }
    if let Some(home) = env_dir("USERPROFILE") {
        candidates.push(home.join(r"scoop\apps\clink\current\clink.bat"));
    }
    candidates.into_iter().find(|p| p.is_file()).or_else(|| find_on_path("clink.bat"))
}

static SHELLS: OnceLock<Vec<ShellProfile>> = OnceLock::new();
static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The installed shells, or `None` while they're still being looked for
/// (the first call starts that in the background; `on_ready` runs when done).
pub fn shells(on_ready: impl FnOnce() + Send + 'static) -> Option<&'static [ShellProfile]> {
    if let Some(found) = SHELLS.get() {
        return Some(found);
    }
    if !STARTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        std::thread::spawn(move || {
            let _ = SHELLS.set(detect());
            on_ready();
        });
    }
    None
}

/// The shell to open when none is picked: the saved choice if it's still
/// installed, else PowerShell 7, Windows PowerShell, or Command Prompt.
pub fn default_shell<'a>(shells: &'a [ShellProfile], saved: Option<&str>) -> Option<&'a ShellProfile> {
    saved
        .and_then(|id| shells.iter().find(|s| s.id == id))
        .or_else(|| ["pwsh", "powershell", "cmd"].iter().find_map(|id| shells.iter().find(|s| s.id == *id)))
        .or_else(|| shells.first())
}

fn detect() -> Vec<ShellProfile> {
    use crate::core::system_paths::{cmd_exe, powershell_exe, system32_dir};
    let mut out = Vec::new();
    let mut add = |id: &str, name: &str, kind: ShellKind, program: PathBuf| {
        out.push(ShellProfile { id: id.into(), name: name.into(), kind, program });
    };
    if cmd_exe().is_file() {
        add("cmd", "Command Prompt", ShellKind::Cmd, cmd_exe());
    }
    if powershell_exe().is_file() {
        add("powershell", "Windows PowerShell", ShellKind::WindowsPowerShell, powershell_exe());
    }
    if let Some(pwsh) = find_pwsh() {
        add("pwsh", "PowerShell 7", ShellKind::PowerShell7, pwsh);
    }
    if let Some(bash) = find_git_bash() {
        add("git-bash", "Git Bash", ShellKind::GitBash, bash);
    }
    let wsl = system32_dir().join("wsl.exe");
    if wsl.is_file() {
        for distro in wsl_distributions() {
            let name = format!("{distro} (WSL)");
            add(&format!("wsl:{distro}"), &name, ShellKind::Wsl(distro), wsl.clone());
        }
    }
    let msys2 = PathBuf::from(r"C:\msys64\usr\bin\bash.exe");
    if msys2.is_file() {
        add("msys2", "MSYS2 Bash", ShellKind::Msys2, msys2);
    }
    let cygwin = PathBuf::from(r"C:\cygwin64\bin\bash.exe");
    if cygwin.is_file() {
        add("cygwin", "Cygwin Bash", ShellKind::Cygwin, cygwin);
    }
    out
}

fn env_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(PathBuf::from)
}

/// The first `name` on PATH, skipping relative entries and the folder the
/// app itself runs from (see `system_paths`).
fn find_on_path(name: &str) -> Option<PathBuf> {
    let own_dir = std::env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf));
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .filter(|dir| dir.is_absolute() && Some(dir) != own_dir.as_ref())
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.symlink_metadata().is_ok())
}

/// PowerShell 7: the MSI/winget install, the preview, the Microsoft Store
/// app alias, or `pwsh.exe` on PATH.
fn find_pwsh() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    for var in ["ProgramW6432", "ProgramFiles"] {
        if let Some(dir) = env_dir(var) {
            candidates.push(dir.join(r"PowerShell\7\pwsh.exe"));
            candidates.push(dir.join(r"PowerShell\7-preview\pwsh.exe"));
        }
    }
    if let Some(local) = env_dir("LOCALAPPDATA") {
        candidates.push(local.join(r"Microsoft\WindowsApps\pwsh.exe"));
    }
    // Store aliases are reparse points `is_file` can't follow.
    candidates.into_iter().find(|p| p.symlink_metadata().is_ok()).or_else(|| find_on_path("pwsh.exe"))
}

/// Git for Windows' `bin\bash.exe` (it sets up the Git Bash environment).
fn find_git_bash() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    for var in ["ProgramW6432", "ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(dir) = env_dir(var) {
            candidates.push(dir.join(r"Git\bin\bash.exe"));
        }
    }
    if let Some(local) = env_dir("LOCALAPPDATA") {
        candidates.push(local.join(r"Programs\Git\bin\bash.exe"));
    }
    // `<Git>\cmd\git.exe` on PATH -> `<Git>\bin\bash.exe`.
    if let Some(git) = find_on_path("git.exe")
        && let Some(root) = git.parent().and_then(Path::parent)
    {
        candidates.push(root.join(r"bin\bash.exe"));
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// Installed WSL distributions, default first, from the registry (no need
/// to start WSL just to list them). Docker Desktop's internal ones are
/// left out.
fn wsl_distributions() -> Vec<String> {
    use windows::Win32::System::Registry::*;
    use windows::core::HSTRING;

    fn read_string(key: HKEY, sub: &str, value: &str) -> Option<String> {
        let mut buf = vec![0u16; 512];
        let mut len = (buf.len() * 2) as u32;
        let status = unsafe {
            RegGetValueW(
                key,
                &HSTRING::from(sub),
                &HSTRING::from(value),
                RRF_RT_REG_SZ,
                None,
                Some(buf.as_mut_ptr().cast()),
                Some(&mut len),
            )
        };
        if status.is_err() {
            return None;
        }
        let chars = (len as usize / 2).saturating_sub(1);
        Some(String::from_utf16_lossy(&buf[..chars.min(buf.len())]))
    }

    const LXSS: &str = r"Software\Microsoft\Windows\CurrentVersion\Lxss";
    let mut key = HKEY::default();
    if unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, &HSTRING::from(LXSS), None, KEY_READ, &mut key) }.is_err() {
        return Vec::new();
    }
    let default = read_string(key, "", "DefaultDistribution");
    let mut found: Vec<(bool, String)> = Vec::new();
    let mut index = 0;
    loop {
        let mut name = [0u16; 256];
        let mut name_len = name.len() as u32;
        let status = unsafe {
            RegEnumKeyExW(key, index, Some(windows::core::PWSTR(name.as_mut_ptr())), &mut name_len, None, None, None, None)
        };
        if status.is_err() {
            break;
        }
        index += 1;
        let guid = String::from_utf16_lossy(&name[..name_len as usize]);
        if let Some(distro) = read_string(key, &guid, "DistributionName") {
            let is_default = default.as_deref() == Some(guid.as_str());
            found.push((is_default, distro));
        }
    }
    unsafe {
        let _ = RegCloseKey(key);
    }
    sort_distributions(found)
}

fn sort_distributions(mut found: Vec<(bool, String)>) -> Vec<String> {
    found.retain(|(_, name)| !name.to_lowercase().starts_with("docker-desktop"));
    found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase())));
    found.into_iter().map(|(_, name)| name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(kind: ShellKind) -> ShellProfile {
        ShellProfile { id: "x".into(), name: "x".into(), kind, program: PathBuf::from(r"C:\x.exe") }
    }

    #[test]
    fn clink_is_injected_with_cmd_quoting() {
        let args = clink_args(Path::new(r"C:\Program Files (x86)\clink\clink.bat"));
        assert_eq!(args, ["/s", "/k", r#"""C:\Program Files (x86)\clink\clink.bat" inject""#]);
    }

    #[test]
    fn launch_arguments() {
        let dir = Path::new(r"C:\Users\Me\My Docs");
        assert!(!profile(ShellKind::PowerShell7).launch(dir).raw_args);
        assert_eq!(profile(ShellKind::PowerShell7).launch(dir).args, ["-NoLogo"]);
        let bash = profile(ShellKind::GitBash).launch(dir);
        assert_eq!(bash.args, ["--login", "-i"]);
        assert_eq!(bash.env, [("CHERE_INVOKING".to_string(), "1".to_string())]);
        let wsl = profile(ShellKind::Wsl("Ubuntu".into())).launch(dir);
        assert_eq!(wsl.args, ["-d", "Ubuntu", "--cd", r"C:\Users\Me\My Docs"]);
    }

    #[test]
    fn cd_commands_quote_paths() {
        let dir = Path::new(r"C:\Bob's Files");
        assert_eq!(profile(ShellKind::Cmd).cd_command(dir), "\x1bcd /d \"C:\\Bob's Files\"\r");
        assert_eq!(
            profile(ShellKind::WindowsPowerShell).cd_command(dir),
            "\x1bSet-Location -LiteralPath 'C:\\Bob''s Files'\r"
        );
        assert_eq!(
            profile(ShellKind::GitBash).cd_command(dir),
            "\x15cd \"$(cygpath -u 'C:\\Bob'\\''s Files')\"\r"
        );
        assert_eq!(
            profile(ShellKind::Wsl("Ubuntu".into())).cd_command(dir),
            "\x15cd \"$(wslpath -u 'C:\\Bob'\\''s Files')\"\r"
        );
    }

    #[test]
    fn default_order_and_distributions() {
        let mut shells = vec![
            ShellProfile { id: "cmd".into(), ..profile(ShellKind::Cmd) },
            ShellProfile { id: "powershell".into(), ..profile(ShellKind::WindowsPowerShell) },
        ];
        assert_eq!(default_shell(&shells, None).unwrap().id, "powershell");
        assert_eq!(default_shell(&shells, Some("cmd")).unwrap().id, "cmd");
        assert_eq!(default_shell(&shells, Some("gone")).unwrap().id, "powershell");
        shells.push(ShellProfile { id: "pwsh".into(), ..profile(ShellKind::PowerShell7) });
        assert_eq!(default_shell(&shells, None).unwrap().id, "pwsh");
        assert_eq!(
            sort_distributions(vec![
                (false, "kali".into()),
                (true, "Ubuntu".into()),
                (false, "docker-desktop".into()),
                (false, "Debian".into()),
            ]),
            ["Ubuntu", "Debian", "kali"]
        );
    }
}
