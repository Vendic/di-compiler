use indexmap::IndexSet;
use rustc_hash::FxHashMap;

/// Merged DI configuration from all di.xml files.
#[derive(Debug, Default, Clone)]
pub struct DiConfig {
    /// preference @for → @type
    pub preferences: FxHashMap<String, String>,
    /// type @name → Vec<Plugin>
    pub plugins: FxHashMap<String, Vec<Plugin>>,
    /// virtualType @name → @type (parent)
    pub virtual_types: FxHashMap<String, VirtualType>,
    /// type @name → TypeConfig
    pub type_configs: FxHashMap<String, TypeConfig>,
    /// Lowercased preference key -> canonical key (for fast case-insensitive lookup)
    pub preference_keys_lc: FxHashMap<String, String>,
    /// Lowercased type config key -> canonical key (for fast case-insensitive lookup)
    pub type_config_keys_lc: FxHashMap<String, String>,
    /// `<type>` and `<virtualType>` names in first-seen order. Until `fold_spellings` runs
    /// they hold the spelling from the file, because `\\X` and `X` are separate nodes.
    pub type_nodes: IndexSet<String>,
    /// Types with plugins, in the order `PluginListGenerator::merge()` first sees them.
    pub plugin_owners: IndexSet<String>,
    /// Virtual types with a `type` attribute in the scope merged last, in file order.
    /// `PluginListGenerator` visits these first when it loads a scope.
    pub scope_virtual_types: Vec<String>,
}

impl DiConfig {
    /// Rebuild case-insensitive lookup indexes.
    pub fn refresh_lookup_indexes(&mut self) {
        self.preference_keys_lc.clear();
        self.preference_keys_lc.reserve(self.preferences.len());
        for key in self.preferences.keys() {
            let lower = key.to_ascii_lowercase();
            self.preference_keys_lc
                .entry(lower)
                .or_insert_with(|| key.clone());
        }

        self.type_config_keys_lc.clear();
        self.type_config_keys_lc.reserve(self.type_configs.len());
        for key in self.type_configs.keys() {
            let lower = key.to_ascii_lowercase();
            self.type_config_keys_lc
                .entry(lower)
                .or_insert_with(|| key.clone());
        }
    }

    /// Insert a preference and update the lowercase index in O(1).
    /// Preserves first-seen canonical casing (same invariant as refresh_lookup_indexes).
    pub fn insert_preference(&mut self, from: String, to: String) {
        let lower = from.to_ascii_lowercase();
        self.preference_keys_lc
            .entry(lower)
            .or_insert_with(|| from.clone());
        self.preferences.insert(from, to);
    }

    /// Insert a type_config entry and update the lowercase index in O(1).
    /// Preserves first-seen canonical casing (same invariant as refresh_lookup_indexes).
    pub fn insert_type_config_key(&mut self, name: &str) {
        let lower = name.to_ascii_lowercase();
        self.type_config_keys_lc
            .entry(lower)
            .or_insert_with(|| name.to_string());
    }
}

#[derive(Debug, Clone)]
pub struct Plugin {
    pub name: String,
    /// Empty when the entry has no `type` attribute: an override of a plugin declared
    /// elsewhere, in an earlier scope or on a parent type.
    pub type_name: String,
    /// `None` when the entry has no `sortOrder` attribute.
    pub sort_order: Option<i32>,
    /// `None` when the entry has no `disabled` attribute.
    pub disabled: Option<bool>,
    /// A later scope added `disabled` to an entry that already had a `type`, so Magento
    /// writes `disabled` after `instance` in this row.
    pub disabled_after_type: bool,
}

impl Plugin {
    pub fn is_disabled(&self) -> bool {
        self.disabled == Some(true)
    }

    /// Disabled entries and entries nobody gave a class never run.
    pub fn is_active(&self) -> bool {
        !self.type_name.is_empty() && !self.is_disabled()
    }

    /// `ObjectManager\Config\Mapper\Dom` writes 0 when `sortOrder` is absent.
    pub fn effective_sort_order(&self) -> i32 {
        self.sort_order.unwrap_or(0)
    }

    /// Merge a later file of the same scope. Magento merges these files as a DOM, so an
    /// absent attribute keeps the earlier value.
    pub fn merge_same_scope(&mut self, later: Plugin) {
        if !later.type_name.is_empty() {
            self.type_name = later.type_name;
        }
        if later.sort_order.is_some() {
            self.sort_order = later.sort_order;
        }
        if later.disabled.is_some() {
            self.disabled = later.disabled;
        }
    }

    /// Merge a later scope. `PluginListGenerator::merge()` uses array_replace_recursive on
    /// mapped rows, and those always hold `sortOrder`, so an absent one resets to 0.
    pub fn merge_later_scope(&mut self, later: Plugin) {
        if !self.type_name.is_empty() && self.disabled.is_none() && later.disabled.is_some() {
            self.disabled_after_type = true;
        }
        let sort_order = later.effective_sort_order();
        self.merge_same_scope(later);
        self.sort_order = Some(sort_order);
    }
}

#[derive(Debug, Clone)]
pub struct VirtualType {
    pub name: String,
    pub type_name: String,
}

#[derive(Debug, Clone, Default)]
pub struct TypeConfig {
    pub shared: Option<bool>,
    pub arguments: Vec<Argument>,
}

#[derive(Debug, Clone)]
pub struct Preference {
    pub for_type: String,
    pub to_type: String,
}

/// Argument value variants matching Magento's `xsi:type` attribute.
#[derive(Debug, Clone)]
pub enum Argument {
    Object {
        name: String,
        value: String,
        shared: Option<bool>,
        /// sortOrder attribute from di.xml (for sorting within parent Array)
        sort_order: i32,
    },
    String {
        name: String,
        value: String,
        sort_order: i32,
    },
    Boolean {
        name: String,
        value: bool,
        sort_order: i32,
    },
    Number {
        name: String,
        value: String,
        sort_order: i32,
    },
    Null {
        name: String,
        sort_order: i32,
    },
    Array {
        name: String,
        items: Vec<Argument>,
        sort_order: i32,
    },
    Init {
        name: String,
        value: String,
        sort_order: i32,
    },
    Const {
        name: String,
        value: String,
        sort_order: i32,
    },
}
