//! `Interpreter\Constant::evaluate()` returns `constant($name)`, whatever its type. So a class
//! constant that holds an array gives an array, and the constructor gets that array.

use std::fs;
use std::path::Path;
use std::process::Command;

fn write(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

/// The arguments of `Acme\On\Model\Thing` in `global.php`, as PHP reads them.
fn compiled_arguments(argument_xml: &str) -> Option<String> {
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
        "app/code/Acme/On/etc/di.xml",
        &format!(
            "<config xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"><type name=\"Acme\\On\\Model\\Thing\"><arguments>{argument_xml}</arguments></type></config>\n"
        ),
    );
    write(
        root,
        "app/code/Acme/On/Model/Settings.php",
        "<?php\nnamespace Acme\\On\\Model;\n\ninterface Settings\n{\n    public const NAME = 'plain';\n    public const FIELDS = ['zulu' => ['one', 'two'], 'alpha' => 3, 'mixed' => [true, null, 1.5]];\n}\n",
    );
    write(
        root,
        "app/code/Acme/On/Model/Thing.php",
        "<?php\nnamespace Acme\\On\\Model;\n\nclass Thing\n{\n    public function __construct($fields, $list = [])\n    {\n    }\n}\n",
    );
    write(
        root,
        "vendor/autoload.php",
        "<?php\nspl_autoload_register(function ($class) {\n    \
         $file = dirname(__DIR__) . '/app/code/' . str_replace('\\\\', '/', $class) . '.php';\n    \
         if (is_file($file)) {\n        require $file;\n    }\n});\n",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_fast-di-compile"))
        .arg("--magento-root")
        .arg(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let dump = Command::new("php")
        .arg("-r")
        .arg("$m = include $argv[1]; echo json_encode($m['arguments']['Acme\\On\\Model\\Thing']);")
        .arg(root.join("generated/metadata/global.php"))
        .output()
        .unwrap();
    Some(String::from_utf8_lossy(&dump.stdout).into_owned())
}

#[test]
fn constant_that_holds_an_array_gives_that_array_in_its_own_order() {
    let Some(arguments) = compiled_arguments(
        r#"<argument name="fields" xsi:type="const">Acme\On\Model\Settings::FIELDS</argument>"#,
    ) else {
        return;
    };
    assert_eq!(
        arguments,
        r#"{"fields":{"_v_":{"zulu":["one","two"],"alpha":3,"mixed":[true,null,1.5]}},"list":{"_v_":[]}}"#
    );
}

#[test]
fn constant_with_an_array_inside_an_array_argument_gives_that_array() {
    let Some(arguments) = compiled_arguments(
        r#"<argument name="fields" xsi:type="array"><item name="inner" xsi:type="const">Acme\On\Model\Settings::FIELDS</item></argument>"#,
    ) else {
        return;
    };
    assert!(
        arguments.contains(r#""inner":{"zulu":["one","two"],"alpha":3"#),
        "{arguments}"
    );
}

#[test]
fn constant_that_holds_a_string_stays_a_string() {
    let Some(arguments) = compiled_arguments(
        r#"<argument name="fields" xsi:type="const">Acme\On\Model\Settings::NAME</argument>"#,
    ) else {
        return;
    };
    assert!(
        arguments.contains(r#""fields":{"_v_":"plain"}"#),
        "{arguments}"
    );
}
