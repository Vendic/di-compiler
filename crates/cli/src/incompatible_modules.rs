//! Modules this compiler does not support. A store that enables one has to use Magento's compile.
//!
//! This is a deliberate difference from Magento, whose compile runs fine with these modules. There
//! is no option to bypass it, because the output would not be what the store asked for.

use rustc_hash::FxHashMap;

struct IncompatibleModule {
    name: &'static str,
    package: &'static str,
    reason: &'static str,
}

const INCOMPATIBLE_MODULES: &[IncompatibleModule] = &[IncompatibleModule {
    name: "Creatuity_Interception",
    package: "creatuity/magento2-interceptors",
    reason:
        "it replaces Magento's Interceptor generator and this compiler does not generate its format",
}];

/// The message for the listed modules that `app/etc/config.php` enables, if there are any.
/// A module that is present but disabled is not in `enabled_modules`, so it does not count.
pub(crate) fn enabled_incompatible_modules(
    enabled_modules: &FxHashMap<String, usize>,
) -> Option<String> {
    let found: Vec<String> = INCOMPATIBLE_MODULES
        .iter()
        .filter(|module| enabled_modules.contains_key(module.name))
        .map(|module| {
            format!(
                "Module {} (package {}) is enabled in app/etc/config.php and is not supported: {}.",
                module.name, module.package, module.reason
            )
        })
        .collect();
    (!found.is_empty()).then(|| {
        format!(
            "{}\nThis store has to use bin/magento setup:di:compile.",
            found.join("\n")
        )
    })
}
