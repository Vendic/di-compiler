//! `ClassReader::getConstructor()` reflects the class of every constructor parameter of every
//! scanned class. A class it cannot find stops the compile with "Impossible to process
//! constructor argument". PHP sees `self` there as a class named `self`, so that fails too.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn write(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

fn compile_with(files: &[(&str, &str)]) -> Option<Output> {
    if Command::new("php").arg("-v").output().is_err() {
        eprintln!("skipped: these tests need php on the PATH");
        return None;
    }
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    write(
        root,
        "app/etc/config.php",
        "<?php\nreturn [\n    'modules' => [\n        'Acme_On' => 1,\n    ],\n];\n",
    );
    write(root, "app/code/Acme/On/registration.php", "<?php\n");
    write(
        root,
        "app/code/Acme/On/etc/module.xml",
        "<config><module name=\"Acme_On\"/></config>\n",
    );
    write(
        root,
        "vendor/autoload.php",
        "<?php\nspl_autoload_register(function ($class) {\n    \
         $file = dirname(__DIR__) . '/app/code/' . str_replace('\\\\', '/', $class) . '.php';\n    \
         if (is_file($file)) {\n        require $file;\n    }\n});\n",
    );
    write(
        root,
        "app/code/Acme/On/Model/Gadget.php",
        "<?php\nnamespace Acme\\On\\Model;\n\nclass Gadget\n{\n}\n",
    );
    for (relative, content) in files {
        write(root, relative, content);
    }
    Some(
        Command::new(env!("CARGO_BIN_EXE_fast-di-compile"))
            .arg("--magento-root")
            .arg(root)
            .output()
            .unwrap(),
    )
}

fn assert_stops(output: &Output, class: &str) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.contains("Impossible to process constructor argument"),
        "{stderr}"
    );
    assert!(stderr.contains(class), "{stderr}");
}

fn assert_compiles(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn thing(body: &str) -> String {
    format!("<?php\nnamespace Acme\\On\\Model;\n\n{body}\n")
}

const THING: &str = "app/code/Acme/On/Model/Thing.php";

#[test]
fn missing_parameter_class_stops_the_compile() {
    let source = thing("class Thing { public function __construct(Gone $gone) {} }");
    let Some(output) = compile_with(&[(THING, &source)]) else {
        return;
    };
    assert_stops(&output, "Acme\\On\\Model\\Thing");
}

#[test]
fn factory_of_a_missing_class_stops_the_compile() {
    let source = thing("class Thing { public function __construct(GoneFactory $gone) {} }");
    let Some(output) = compile_with(&[(THING, &source)]) else {
        return;
    };
    assert_stops(&output, "Acme\\On\\Model\\Thing");
}

#[test]
fn parameter_typed_self_stops_the_compile() {
    let source = thing("class Thing { public function __construct(self $other) {} }");
    let Some(output) = compile_with(&[(THING, &source)]) else {
        return;
    };
    assert_stops(&output, "Acme\\On\\Model\\Thing");
}

#[test]
fn missing_class_in_an_inherited_constructor_stops_the_compile() {
    let parent = thing("class Base { public function __construct(Gone $gone) {} }");
    let child = thing("class Thing extends Base {}");
    let Some(output) = compile_with(&[
        ("app/code/Acme/On/Model/Base.php", &parent),
        (THING, &child),
    ]) else {
        return;
    };
    assert_stops(&output, "Acme\\On\\Model\\");
}

#[test]
fn factory_and_proxy_of_an_existing_class_compile() {
    let source = thing(
        "class Thing { public function __construct(GadgetFactory $f, Gadget\\Proxy $p, ?Gadget $g = null, int $n = 1, $any = null) {} }",
    );
    let Some(output) = compile_with(&[(THING, &source)]) else {
        return;
    };
    assert_compiles(&output);
}

#[test]
fn union_type_with_a_missing_class_compiles_because_magento_skips_unions() {
    let source = thing("class Thing { public function __construct(Gone|Gadget $either) {} }");
    let Some(output) = compile_with(&[(THING, &source)]) else {
        return;
    };
    assert_compiles(&output);
}

#[test]
fn missing_class_in_a_plain_method_compiles() {
    let source = thing("class Thing { public function run(Gone $gone) {} }");
    let Some(output) = compile_with(&[(THING, &source)]) else {
        return;
    };
    assert_compiles(&output);
}

#[test]
fn class_the_file_does_not_declare_stops_the_compile() {
    let source = thing("if (class_exists(\\Acme\\Gone::class)) {\n    class Thing {}\n}");
    let Some(output) = compile_with(&[(THING, &source)]) else {
        return;
    };
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("Acme\\On\\Model\\Thing"), "{stderr}");
}
