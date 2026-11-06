//! Checks every di.xml the way Magento reads it.
//!
//! `Config\Reader\Filesystem::_readFiles()` hands each file to `Config\Dom::_initDom()`, which
//! stops the compile when `DOMDocument::loadXML()` fails. The Rust parser accepts more than
//! libxml does, so PHP answers this question. It runs beside the rest of the compile.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread::JoinHandle;

const CHECK_DI_XML_PHP: &str = r#"
ini_set('display_errors', '0');
libxml_use_internal_errors(true);
foreach (file('php://stdin', FILE_IGNORE_NEW_LINES) as $file) {
    libxml_clear_errors();
    try {
        $loaded = (new DOMDocument())->loadXML((string) file_get_contents($file));
        $messages = [];
        foreach (libxml_get_errors() as $error) {
            $messages[] = trim($error->message) . "\nLine: " . $error->line;
        }
    } catch (\Throwable $e) {
        $loaded = false;
        $messages = [$e->getMessage()];
    }
    if (!$loaded) {
        echo "invalid\n", 'The XML in file "', $file, "\" is invalid:\n", implode("\n", $messages),
            "\nVerify the XML and try again.\n";
        exit(0);
    }
}
echo "valid\n";
"#;

pub(crate) struct DiXmlCheck(JoinHandle<Result<(), String>>);

impl DiXmlCheck {
    pub(crate) fn start(files: Vec<PathBuf>, php_bin: String) -> Self {
        Self(std::thread::spawn(move || check(&files, &php_bin)))
    }

    /// The message Magento stops with, if it stops.
    pub(crate) fn finish(self) -> Result<(), String> {
        self.0
            .join()
            .unwrap_or_else(|_| Err("The di.xml check ended unexpectedly".to_string()))
    }
}

fn check(files: &[PathBuf], php_bin: &str) -> Result<(), String> {
    let could_not_run = |reason: String| format!("Could not check di.xml files in PHP: {reason}");
    let spawned = Command::new(php_bin)
        .args(["-d", "display_errors=0", "-r", CHECK_DI_XML_PHP])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        // Without a PHP binary nothing can run in PHP, here or in any other phase.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            log::warn!("Could not check di.xml files: no PHP binary at {php_bin}");
            return Ok(());
        }
        Err(e) => return Err(could_not_run(e.to_string())),
    };
    let mut input = String::new();
    for file in files {
        input.push_str(&file.to_string_lossy());
        input.push('\n');
    }
    let mut stdin = child.stdin.take().expect("stdin is piped");
    // PHP reads all of stdin first, so this cannot block on PHP's output.
    stdin
        .write_all(input.as_bytes())
        .map_err(|e| could_not_run(e.to_string()))?;
    drop(stdin);
    let output = child
        .wait_with_output()
        .map_err(|e| could_not_run(e.to_string()))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    match stdout.split_once('\n') {
        Some(("valid", _)) => Ok(()),
        Some(("invalid", message)) => Err(message.trim_end().to_string()),
        _ => Err(could_not_run(format!(
            "no answer, PHP exit code {:?}",
            output.status.code()
        ))),
    }
}
