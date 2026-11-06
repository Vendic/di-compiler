//! Loads every scanned class in PHP, as `ClassesScanner::includeClass()` does during compile,
//! and reads its constructor, as `ClassReader::getConstructor()` does.
//!
//! A class file that PHP cannot load stops `setup:di:compile`. Most reasons cannot be seen from
//! the source: a parent that is not on the autoload path, a signature that does not fit the
//! parent, a syntax error. So PHP answers. The workers run beside the rest of the compile.
//!
//! The workers load only part of the classes each, in their own order, and they stand in for a
//! generated class with an empty one. A failure there can be caused by that, so a failure is
//! only final after `confirm` repeats it in one process, after everything else has loaded and
//! with the generated code on disk. That costs time only when a compile is about to stop.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::JoinHandle;

use serde::Deserialize;

const CLASS_CHECK_PHP: &str = r#"
ini_set('display_errors', '0');
[, $mode, $root, $generated] = $argv;
$entries = [];
foreach (file('php://stdin', FILE_IGNORE_NEW_LINES) as $line) {
    $entries[] = explode("\t", $line);
}
$emit = static function (array $row): void {
    fwrite(STDOUT, json_encode($row, JSON_INVALID_UTF8_SUBSTITUTE) . "\n");
};
$current = null;
$finished = false;
register_shutdown_function(static function () use (&$current, &$finished, $emit): void {
    if ($finished) {
        return;
    }
    $last = error_get_last();
    $emit(['i' => $current, 'error' => $last !== null && ($last['type'] & (E_ERROR | E_PARSE | E_CORE_ERROR | E_COMPILE_ERROR))
        ? 'There is an error in ' . $last['file'] . ' at line: ' . $last['line'] . ': ' . $last['message']
        : 'The file ended the PHP process']);
});
ob_start();
require is_file($root . '/app/bootstrap.php') ? $root . '/app/bootstrap.php' : $root . '/vendor/autoload.php';
while (ob_get_level() > 0) {
    ob_end_clean();
}
ini_set('display_errors', '0');
ini_set('memory_limit', '-1');
// `Code\Generator\Autoloader` creates these classes on demand when the source class exists.
spl_autoload_register(static function (string $class) use ($generated): void {
    $file = $generated . '/' . str_replace('\\', '/', $class) . '.php';
    if (is_file($file)) {
        require $file;
        return;
    }
    foreach (['\\Proxy', '\\Interceptor', '\\ProxyDeferred', '\\Remote', '\\Logger', 'Factory', 'Mapper',
              'Persistor', 'Repository', 'Converter', 'SearchResults', 'ExtensionInterface', 'Extension'] as $suffix) {
        if (!str_ends_with($class, $suffix)) {
            continue;
        }
        $source = substr($class, 0, -strlen($suffix));
        foreach ([$source, $source . 'Interface'] as $name) {
            if ($name === '' || $name === 'Interface' || (!class_exists($name) && !interface_exists($name))) {
                continue;
            }
            $split = strrpos($class, '\\');
            $namespace = $split === false ? '' : 'namespace ' . substr($class, 0, $split) . '; ';
            $short = substr($class, $split === false ? 0 : $split + 1);
            try {
                eval($namespace . ($suffix === 'ExtensionInterface' ? 'interface ' : 'class ') . $short . ' {}');
            } catch (\Throwable) {
            }
            return;
        }
    }
});
$scanner = 'Magento\\Setup\\Module\\Di\\Code\\Reader\\FileClassScanner';
// `extract()` in `ClassesScanner` takes the class name from `FileClassScanner`.
$className = static function (string $file) use ($scanner): string {
    return class_exists($scanner) ? (new $scanner($file))->getClassName() : '';
};
$load = static function (string $class, string $file): void {
    if (!class_exists($class)) {
        require_once $file;
    }
};
// `Decorator\Area::getList()` calls `ClassReader::getConstructor()` on every scanned class.
$reader = 'Magento\\Framework\\Code\\Reader\\ClassReader';
$reader = class_exists($reader) ? new $reader() : null;
$readConstructor = static function (string $class) use ($reader): void {
    if ($reader !== null) {
        $reader->getConstructor($class);
        return;
    }
    foreach ((new ReflectionClass($class))->getConstructor()?->getParameters() ?? [] as $parameter) {
        $type = $parameter->getType();
        if ($type instanceof ReflectionNamedType && !$type->isBuiltin()) {
            try {
                new ReflectionClass($type->getName());
            } catch (ReflectionException $e) {
                throw new ReflectionException('Impossible to process constructor argument ' . $parameter . ' of ' . $class . ' class', 0, $e);
            }
        }
    }
};
$check = static function (string $class, string $file) use ($load, $readConstructor): void {
    $load($class, $file);
    $readConstructor($class);
};
$describe = static function (\Throwable $e): string {
    return $e instanceof ReflectionException
        ? $e->getMessage()
        : 'There is an error in ' . $e->getFile() . ' at line: ' . $e->getLine() . ': ' . $e->getMessage();
};
$level = ob_get_level();
if ($mode === 'check') {
    foreach ($entries as $i => [$class, $file]) {
        $current = $i;
        ob_start();
        try {
            $class = $class !== '' ? $class : $className($file);
            if ($class !== '') {
                $check($class, $file);
            }
        } catch (\Throwable $e) {
            $emit(['i' => $i, 'error' => $describe($e)]);
        }
        while (ob_get_level() > $level) {
            ob_end_clean();
        }
    }
} else {
    // The failed classes come last, so everything they could depend on has loaded by then.
    $registrar = 'Magento\\Framework\\Component\\ComponentRegistrar';
    $scanned = null;
    if (class_exists($registrar)) {
        $components = new $registrar();
        $scanned = [$root . '/setup'];
        foreach (['module', 'library'] as $type) {
            array_push($scanned, ...array_values($components->getPaths($type)));
        }
        $scanned = array_map(static fn (string $path): string => (realpath($path) ?: $path) . '/', $scanned);
    }
    foreach ($entries as $i => [$class, $file, $failed]) {
        $current = $i;
        ob_start();
        if ($failed === '') {
            try {
                if ($class !== '') {
                    $load($class, $file);
                }
            } catch (\Throwable) {
            }
        } else {
            $real = realpath($file) ?: $file;
            $registered = $scanned === null || array_filter($scanned, static fn (string $path): bool => str_starts_with($real, $path)) !== [];
            $name = class_exists($scanner) ? $className($file) : $class;
            if ($registered && $name !== '') {
                try {
                    $check($name, $file);
                } catch (\Throwable $e) {
                    $emit(['i' => $i, 'error' => $describe($e)]);
                    $finished = true;
                    exit(0);
                }
            }
        }
        while (ob_get_level() > $level) {
            ob_end_clean();
        }
    }
}
$finished = true;
$emit(['done' => true]);
"#;

/// What the extractor found in one file. An empty class means it could not read the file.
pub(crate) struct ScannedFile {
    pub(crate) class: String,
    pub(crate) path: PathBuf,
}

#[derive(Deserialize)]
struct Row {
    i: Option<usize>,
    error: Option<String>,
    done: Option<bool>,
}

struct Failure {
    index: usize,
    message: String,
}

pub(crate) struct ClassChecks {
    entries: Vec<ScannedFile>,
    shards: Vec<JoinHandle<Result<Vec<Failure>, String>>>,
    root: PathBuf,
    code_root: PathBuf,
    php_bin: String,
}

fn entry_line(entry: &ScannedFile, failed: bool) -> String {
    format!(
        "{}\t{}\t{}\n",
        entry.class,
        entry.path.display(),
        if failed { "1" } else { "" }
    )
}

impl ClassChecks {
    pub(crate) fn start(
        mut entries: Vec<ScannedFile>,
        root: &Path,
        code_root: &Path,
        php_bin: &str,
        jobs: usize,
    ) -> Self {
        // Without an autoloader this is not a Magento store, and PHP can load nothing.
        if !root.join("vendor/autoload.php").is_file() {
            entries.clear();
        }
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        // Neighbours share parents, so a block of neighbours loads fewer classes than a spread.
        let size = entries.len().div_ceil(jobs.max(1)).max(1);
        let mut shards = Vec::new();
        for (block, chunk) in entries.chunks(size).enumerate() {
            let lines: Vec<String> = chunk.iter().map(|e| entry_line(e, false)).collect();
            let offset = block * size;
            let root = root.to_path_buf();
            let code_root = code_root.to_path_buf();
            let php_bin = php_bin.to_string();
            shards.push(std::thread::spawn(move || {
                run_shard(&lines, offset, &root, &code_root, &php_bin)
            }));
        }
        Self {
            entries,
            shards,
            root: root.to_path_buf(),
            code_root: code_root.to_path_buf(),
            php_bin: php_bin.to_string(),
        }
    }

    /// Waits for the workers. The error is the message Magento stops with.
    pub(crate) fn finish(self) -> Result<(), String> {
        let mut failures = Vec::new();
        for shard in self.shards {
            failures.extend(
                shard
                    .join()
                    .unwrap_or_else(|_| Err("The class check ended unexpectedly".to_string()))?,
            );
        }
        if failures.is_empty() {
            return Ok(());
        }
        failures.sort_by_key(|failure| failure.index);
        confirm(
            &self.entries,
            &failures,
            &self.root,
            &self.code_root,
            &self.php_bin,
        )
    }
}

enum Run {
    NoPhp,
    Rows(Vec<Row>, Option<i32>),
}

fn run_php(
    mode: &str,
    input: &str,
    root: &Path,
    code_root: &Path,
    php_bin: &str,
) -> Result<Run, String> {
    let spawned = Command::new(php_bin)
        .args(["-d", "display_errors=0", "-r", CLASS_CHECK_PHP, "--", mode])
        .arg(root)
        .arg(code_root)
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        // Without a PHP binary nothing can run in PHP, here or in any other phase.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            log::warn!("Could not check classes: no PHP binary at {php_bin}");
            return Ok(Run::NoPhp);
        }
        Err(e) => return Err(format!("Could not check classes in PHP: {e}")),
    };
    let mut stdin = child.stdin.take().expect("stdin is piped");
    // PHP reads all of stdin before it does anything else.
    stdin
        .write_all(input.as_bytes())
        .map_err(|e| format!("Could not check classes in PHP: {e}"))?;
    drop(stdin);
    let output = child
        .wait_with_output()
        .map_err(|e| format!("Could not check classes in PHP: {e}"))?;
    let rows = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<Row>(line).ok())
        .collect();
    Ok(Run::Rows(rows, output.status.code()))
}

fn run_shard(
    lines: &[String],
    offset: usize,
    root: &Path,
    code_root: &Path,
    php_bin: &str,
) -> Result<Vec<Failure>, String> {
    let mut failures = Vec::new();
    let mut start = 0;
    while start < lines.len() {
        let run = run_php("check", &lines[start..].concat(), root, code_root, php_bin)?;
        let Run::Rows(rows, code) = run else {
            return Ok(failures);
        };
        let done = rows.iter().any(|row| row.done == Some(true));
        let mut last_failed = None;
        for row in rows {
            if let (Some(i), Some(message)) = (row.i, row.error) {
                failures.push(Failure {
                    index: offset + start + i,
                    message,
                });
                last_failed = Some(start + i);
            }
        }
        if done {
            break;
        }
        // The process died on this class. The rest of the block still has to run.
        let Some(failed) = last_failed else {
            return Err(format!(
                "PHP stopped while it loaded the scanned classes, exit code {code:?}"
            ));
        };
        start = failed + 1;
    }
    Ok(failures)
}

fn confirm(
    entries: &[ScannedFile],
    failures: &[Failure],
    root: &Path,
    code_root: &Path,
    php_bin: &str,
) -> Result<(), String> {
    let is_failed = |index: usize| failures.iter().any(|failure| failure.index == index);
    let mut order: Vec<usize> = (0..entries.len()).filter(|i| !is_failed(*i)).collect();
    let passed = order.len();
    order.extend(failures.iter().map(|failure| failure.index));
    let input: String = order
        .iter()
        .enumerate()
        .map(|(position, index)| entry_line(&entries[*index], position >= passed))
        .collect();
    let Run::Rows(rows, code) = run_php("confirm", &input, root, code_root, php_bin)? else {
        return Ok(());
    };
    if rows.iter().any(|row| row.done == Some(true)) {
        for failure in failures {
            log::warn!(
                "{} passes when everything else has loaded: {}",
                entries[failure.index].path.display(),
                failure.message
            );
        }
        return Ok(());
    }
    let Some(row) = rows.into_iter().find(|row| row.error.is_some()) else {
        return Err(format!(
            "PHP stopped while it loaded the scanned classes, exit code {code:?}"
        ));
    };
    let index = row.i.and_then(|i| order.get(i).copied());
    let message = row.error.unwrap_or_default();
    Err(match index {
        Some(index) if entries[index].class.is_empty() => {
            format!("{}: {message}", entries[index].path.display())
        }
        Some(index) => format!(
            "Class {} in {}: {message}",
            entries[index].class,
            entries[index].path.display()
        ),
        None => message,
    })
}
