//! A module on the incompatibility list stops the compile when `app/etc/config.php` enables it.
//! This is a deliberate difference from Magento, whose own compile runs fine with such a module.
//! The stop comes first, so the output of an earlier compile stays as it was.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const MODULE: &str = "Creatuity_Interception";

fn write(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

/// A store with the module on disk. `config_php` decides whether it counts as used.
fn store(config_php: &str) -> Option<tempfile::TempDir> {
    if Command::new("php").arg("-v").output().is_err() {
        eprintln!("skipped: these tests need php on the PATH");
        return None;
    }
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    write(root, "app/etc/config.php", config_php);
    for (folder, module) in [("Acme/On", "Acme_On"), ("Creatuity/Interception", MODULE)] {
        write(
            root,
            &format!("app/code/{folder}/registration.php"),
            "<?php\n",
        );
        write(
            root,
            &format!("app/code/{folder}/etc/module.xml"),
            &format!("<config><module name=\"{module}\"/></config>\n"),
        );
    }
    write(
        root,
        "vendor/autoload.php",
        "<?php\nspl_autoload_register(function ($class) {\n    \
         $file = dirname(__DIR__) . '/app/code/' . str_replace('\\\\', '/', $class) . '.php';\n    \
         if (is_file($file)) {\n        require $file;\n    }\n});\n",
    );
    Some(tmp)
}

fn config(status: &str) -> String {
    format!("<?php\nreturn [\n    'modules' => [\n        'Acme_On' => 1,\n{status}    ],\n];\n")
}

fn enabled() -> String {
    config(&format!("        '{MODULE}' => 1,\n"))
}

fn compile(root: &Path, extra: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fast-di-compile"))
        .arg("--magento-root")
        .arg(root)
        .args(extra)
        .output()
        .unwrap()
}

fn tree(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in fs::read_dir(folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                folders.push(path);
            } else {
                let content = fs::read(&path).unwrap();
                files.insert(path, content);
            }
        }
    }
    files
}

fn assert_stopped(output: &Output) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    for expected in [
        MODULE,
        "creatuity/magento2-interceptors",
        "Interceptor generator",
        "bin/magento setup:di:compile",
    ] {
        assert!(stderr.contains(expected), "no {expected} in: {stderr}");
    }
}

#[test]
fn enabled_module_stops_and_leaves_earlier_output_alone() {
    let Some(tmp) = store(&enabled()) else {
        return;
    };
    let root = tmp.path();
    write(
        root,
        "generated/metadata/global.php",
        "<?php return ['old' => true];\n",
    );
    write(
        root,
        "generated/code/Acme/On/Old/Interceptor.php",
        "<?php\n",
    );
    let before = tree(&root.join("generated"));
    assert_stopped(&compile(root, &[]));
    assert_eq!(tree(&root.join("generated")), before);
}

#[test]
fn enabled_module_stops_before_the_output_folder_exists() {
    let Some(tmp) = store(&enabled()) else {
        return;
    };
    assert_stopped(&compile(tmp.path(), &[]));
    assert!(!tmp.path().join("generated").exists());
}

#[test]
fn enabled_module_in_compact_config_stops() {
    let compact = format!("<?php return ['modules' => ['Acme_On' => 1, '{MODULE}' => 1]];\n");
    let Some(tmp) = store(&compact) else {
        return;
    };
    assert_stopped(&compile(tmp.path(), &[]));
}

#[test]
fn enabled_module_stops_when_php_cannot_read_the_config() {
    let Some(tmp) = store(&enabled()) else {
        return;
    };
    assert_stopped(&compile(
        tmp.path(),
        &["--fallback-php", "/nonexistent/php"],
    ));
}

#[test]
fn disabled_module_does_not_stop_the_compile() {
    let Some(tmp) = store(&config(&format!("        '{MODULE}' => 0,\n"))) else {
        return;
    };
    let output = compile(tmp.path(), &[]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(!stderr.contains(MODULE), "{stderr}");
    assert!(tmp.path().join("generated/metadata/global.php").exists());
}

#[test]
fn module_missing_from_the_config_does_not_stop_the_compile() {
    let Some(tmp) = store(&config("")) else {
        return;
    };
    let output = compile(tmp.path(), &[]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(tmp.path().join("generated/metadata/global.php").exists());
}
