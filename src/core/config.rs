use std::env;
use std::path::PathBuf;

use log::{info, warn};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Port configuration: "auto" (random port) or a fixed port number (1-65535).
#[derive(Debug, Clone)]
pub enum PortConfig {
    Auto,
    Fixed(u16),
}

impl Default for PortConfig {
    fn default() -> Self {
        Self::Auto
    }
}

/// The application-level configuration merged from multiple sources.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub port: PortConfig,
    #[serde(default)]
    pub nickname: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: PortConfig::Auto,
            nickname: None,
        }
    }
}

impl Config {
    /// Load configuration by merging sources with ascending priority:
    ///   1. default values
    ///   2. `$HOME/.config/lantype/config.json` (global)
    ///   3. `./config.json` (local, project/cwd)
    ///
    /// Merging is shallow at the top level — keys in higher-priority sources
    /// replace those in lower-priority sources entirely.
    pub fn load() -> Self {
        let mut merged = serde_json::json!({});

        // Global config: $HOME/.config/lantype/config.json
        if let Some(global_path) = global_config_path() {
            if global_path.exists() {
                match std::fs::read_to_string(&global_path) {
                    Ok(content) => match serde_json::from_str::<Value>(&content) {
                        Ok(val) => merge(&mut merged, val),
                        Err(e) => warn!("Failed to parse global config at {}: {e}", global_path.display()),
                    },
                    Err(e) => warn!("Failed to read global config at {}: {e}", global_path.display()),
                }
            }
        }

        // Local config: ./config.json (current working directory)
        let local_path = PathBuf::from("config.json");
        if local_path.exists() {
            match std::fs::read_to_string(&local_path) {
                Ok(content) => match serde_json::from_str::<Value>(&content) {
                    Ok(val) => merge(&mut merged, val),
                    Err(e) => warn!("Failed to parse local config at {}: {e}", local_path.display()),
                },
                Err(e) => warn!("Failed to read local config at {}: {e}", local_path.display()),
            }
        }

        serde_json::from_value(merged).unwrap_or_else(|e| {
            warn!("Failed to deserialize merged config, using defaults: {e}");
            Config::default()
        })
    }
}

// ---- Device name resolution ----

static ADJECTIVES: &[&str] = &[
    "可爱的", "危险的", "暴躁的", "忧伤的", "偷偷的", "发光的", "孤独的", "甜蜜的", "易碎的", "沉默的",
    "逃跑的", "失眠的", "叛逆的", "害羞的", "爆炸的", "融化的", "漂浮的", "醉酒的", "生锈的", "透明的",
    "迷路的", "疯狂的", "流泪的", "发热的", "冬眠的", "迷幻的", "尖叫的", "坠落的", "燃烧的", "冰冻的",
    "颤抖的", "偷懒的", "撒娇的", "说谎的", "腐烂的", "饥饿的", "伪装的", "焦虑的", "迟钝的", "流浪的",
    "沉睡的", "微笑的", "哭泣的", "愤怒的", "绝望的", "好奇的", "贪婪的", "傲慢的", "谦虚的", "自卑的",
    "自信的", "迷茫的", "清醒的", "麻木的", "狂热的", "温柔的", "冷酷的", "热烈的", "冰冷的", "潮湿的",
    "干燥的", "拥挤的", "空旷的", "喧闹的", "安静的", "忙碌的", "懒惰的", "勤奋的", "愚蠢的", "聪明的",
    "笨拙的", "灵巧的", "粗鲁的", "优雅的", "丑陋的", "美丽的", "平凡的", "特别的", "普通的", "稀有的",
    "常见的", "古老的", "年轻的", "成熟的", "幼稚的", "天真的", "狡猾的", "善良的", "邪恶的", "正义的",
    "黑暗的", "光明的", "混合的", "纯粹的", "混沌的",
];

static FRUITS: &[&str] = &[
    "桃子", "柠檬", "草莓", "樱桃", "芒果", "葡萄", "西瓜", "菠萝", "荔枝", "蓝莓",
    "苹果", "橙子", "柚子", "石榴", "猕猴桃", "火龙果", "百香果", "椰子", "榴莲", "山竹",
    "哈密瓜", "香瓜", "木瓜", "杨桃", "蜜瓜", "菠萝蜜", "牛油果", "莲雾", "山莓", "黑莓",
    "树莓", "蓝莓", "无花果", "枇杷", "杨梅", "龙眼", "红毛丹", "番石榴", "橄榄", "李子",
    "杏子", "枣子", "柿子", "山楂", "海棠果", "沙果", "葡萄柚", "金橘", "青橘", "柑橘",
    "丑橘", "粑粑柑", "砂糖橘", "沃柑", "蜜橘", "血橙", "脐橙", "冰糖橙", "香水梨", "雪梨",
    "鸭梨", "丰水梨", "库尔勒香梨", "水晶梨", "青苹果", "红富士", "嘎啦果", "蛇果", "青提", "红提",
    "黑提", "巨峰葡萄", "阳光玫瑰", "马奶葡萄", "冬枣", "青枣", "贵妃芒", "凯特芒", "台农芒", "大青芒",
    "小台芒", "金枕榴莲", "猫山王", "苏丹王", "红肉菠萝蜜", "黄肉菠萝蜜", "白心火龙果", "红心火龙果", "黄皮", "释迦果",
    "人参果", "姑娘果", "酸角", "罗望子", "桑葚", "覆盆子", "蔓越莓", "西梅", "青梅", "脆梅",
    "话梅", "黄桃", "油桃", "蟠桃", "水蜜桃", "毛桃", "黑布林", "红布林", "青李",
];

fn generate_random_name() -> String {
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as usize;
    let adj = ADJECTIVES[seed % ADJECTIVES.len()];
    let fruit = FRUITS[(seed.wrapping_mul(17).wrapping_add(31)) % FRUITS.len()];
    format!("{adj}{fruit}")
}

/// Resolve the device name:
/// - If `config.nickname` is set, use it.
/// - Otherwise, generate a random nickname, persist it into the global config file
///   (preserving existing keys like `port`), and return it.
pub fn resolve_device_name(config: &Config) -> String {
    if let Some(ref nickname) = config.nickname {
        return nickname.clone();
    }

    let name = generate_random_name();
    let Some(global_path) = global_config_path() else {
        warn!("No HOME directory found, cannot persist random nickname");
        return name;
    };

    // Read existing global config (if any) to merge
    let mut existing: Value = if global_path.exists() {
        std::fs::read_to_string(&global_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    // Insert or overwrite the nickname key only
    if let Value::Object(ref mut map) = existing {
        map.insert(
            "nickname".to_string(),
            Value::String(name.clone()),
        );
    }

    // Ensure parent directory exists
    if let Some(parent) = global_path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            warn!("Failed to create config directory {}: {e}", parent.display());
            return name;
        }
    }

    let content = serde_json::to_string_pretty(&existing).unwrap_or_else(|_| {
        format!("{{\"nickname\":\"{}\"}}", name)
    });

    match std::fs::write(&global_path, &content) {
        Ok(()) => info!("Persisted random nickname \"{name}\" to {}", global_path.display()),
        Err(e) => warn!("Failed to write nickname to {}: {e}", global_path.display()),
    }

    name
}

// ---- Serialisation helpers ----

impl<'de> Deserialize<'de> for PortConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match value {
            Value::String(s) if s == "auto" => Ok(PortConfig::Auto),
            Value::Number(n) => {
                let port = n.as_u64().ok_or_else(|| {
                    serde::de::Error::custom("expected a positive integer for port")
                })?;
                if port == 0 || port > 65535 {
                    return Err(serde::de::Error::custom(
                        "port must be in range 1-65535",
                    ));
                }
                Ok(PortConfig::Fixed(port as u16))
            }
            _ => Err(serde::de::Error::custom(
                "expected \"auto\" or a port number (1-65535)",
            )),
        }
    }
}

impl Serialize for PortConfig {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            PortConfig::Auto => serializer.serialize_str("auto"),
            PortConfig::Fixed(port) => serializer.serialize_u16(*port),
        }
    }
}

// ---- Helpers ----

/// Shallow top-level merge: keys from `override_val` replace those in `base`.
fn merge(base: &mut Value, override_val: Value) {
    if let (Value::Object(base_map), Value::Object(override_map)) = (base, override_val) {
        for (k, v) in override_map {
            base_map.insert(k, v);
        }
    }
}

/// Returns the path to the global config file:
/// `$HOME/.config/lantype/config.json`
pub(crate) fn global_config_path() -> Option<PathBuf> {
    env::var("HOME").ok().map(|home| {
        PathBuf::from(home)
            .join(".config")
            .join("lantype")
            .join("config.json")
    })
}