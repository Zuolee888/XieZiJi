//! Open template interface: local providers now; MCP adapters can implement it later.
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Template {
    pub id: String,
    pub name: String,
    pub description: String,
    pub width_mm: f64,
    pub height_mm: f64,
    pub margin_mm: f64,
    pub font_size_mm: f64,
    pub letter_spacing_mm: f64,
    pub lines: usize,
    /// 空格宽度（毫米）；缺省时由前端按字号折算。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub space_mm: Option<f64>,
}

pub trait TemplateProvider {
    fn list(&self) -> Result<Vec<Template>, String>;
}

pub struct TemplateStore {
    directory: PathBuf,
}
impl TemplateStore {
    pub fn new() -> Self {
        let base = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        Self {
            directory: base.join("OpenInkStudio").join("templates"),
        }
    }
    pub(crate) fn validate(t: &Template) -> Result<(), String> {
        if t.id.is_empty()
            || t.id.len() > 96
            || !t
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err("模板ID仅允许英文字母、数字、下划线和短横线".into());
        }
        if t.name.trim().is_empty()
            || ![
                t.width_mm,
                t.height_mm,
                t.margin_mm,
                t.font_size_mm,
                t.letter_spacing_mm,
            ]
            .iter()
            .all(|v| v.is_finite())
            || t.width_mm <= 0.
            || t.height_mm <= 0.
            || t.width_mm > 1000.
            || t.height_mm > 1000.
            || t.margin_mm < 0.
            || t.margin_mm * 2. >= t.width_mm.min(t.height_mm)
            || t.font_size_mm <= 0.
            || t.letter_spacing_mm < 0.
            || t.lines == 0
            || t.lines > 1000
        {
            return Err("模板尺寸、边距或文字参数无效".into());
        }
        Ok(())
    }
    pub fn list(&self) -> Result<Vec<Template>, String> {
        <Self as TemplateProvider>::list(self)
    }
    pub fn save(&self, t: &Template) -> Result<(), String> {
        Self::validate(t)?;
        if t.id.starts_with("builtin-") {
            return Err("请另存为自定义模板".into());
        }
        fs::create_dir_all(&self.directory).map_err(|e| e.to_string())?;
        let bytes = serde_json::to_vec_pretty(t).map_err(|e| e.to_string())?;
        fs::write(self.directory.join(format!("{}.json", t.id)), bytes).map_err(|e| e.to_string())
    }
    pub fn delete(&self, id: &str) -> Result<(), String> {
        if id.starts_with("builtin-")
            || id.is_empty()
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err("不能删除内置模板或非法ID".into());
        }
        fs::remove_file(self.directory.join(format!("{id}.json"))).map_err(|e| e.to_string())
    }
}
impl TemplateProvider for TemplateStore {
    fn list(&self) -> Result<Vec<Template>, String> {
        let mut items = vec![
            Template {
                id: "builtin-note".into(),
                name: "日常笔记".into(),
                description: "160 × 190 mm / 适合常规书写".into(),
                width_mm: 160.,
                height_mm: 190.,
                margin_mm: 8.,
                font_size_mm: 7.5,
                letter_spacing_mm: 1.,
                lines: 18,
                space_mm: None,
            },
            Template {
                id: "builtin-square".into(),
                name: "方形画布".into(),
                description: "190 × 190 mm / 插画与自由排版".into(),
                width_mm: 190.,
                height_mm: 190.,
                margin_mm: 8.,
                font_size_mm: 10.,
                letter_spacing_mm: 1.,
                lines: 15,
                space_mm: None,
            },
            Template {
                id: "builtin-a4".into(),
                name: "A4 文档".into(),
                description: "210 × 297 mm / 排版预览；实际轨迹需在设备范围内".into(),
                width_mm: 210.,
                height_mm: 297.,
                margin_mm: 15.,
                font_size_mm: 7.5,
                letter_spacing_mm: 1.,
                lines: 24,
                space_mm: None,
            },
            // 与出厂样板（样板工程 starter-physics.inkproj）配套的版面：
            // 换一台电脑、样板被用户另存之后，仍能在模板列表里找回同一套版心。
            Template {
                id: "builtin-physics".into(),
                name: "自物理实验报告".into(),
                description: "210 × 297 mm / 样板工程的版心（36 行 / 边距 20 mm）".into(),
                width_mm: 210.,
                height_mm: 297.,
                margin_mm: 20.,
                font_size_mm: 10.,
                letter_spacing_mm: 1.,
                lines: 36,
                space_mm: None,
            },
            Template {
                id: "builtin-grid".into(),
                name: "自学院稿纸".into(),
                description: "190 × 265 mm / 稿纸版心（21 行 / 边距 19 mm）".into(),
                width_mm: 190.,
                height_mm: 265.,
                margin_mm: 19.,
                font_size_mm: 10.,
                letter_spacing_mm: 1.,
                lines: 21,
                space_mm: None,
            },
        ];
        if self.directory.exists() {
            // One unreadable or corrupt file must not hide every other saved
            // template, so skip bad entries instead of failing the whole list.
            let mut seen = std::collections::HashSet::new();
            for entry in fs::read_dir(&self.directory).map_err(|e| e.to_string())? {
                let Ok(entry) = entry else { continue };
                let path = entry.path();
                if path.extension().and_then(|x| x.to_str()) != Some("json") {
                    continue;
                }
                let Ok(data) = fs::read(&path) else {
                    eprintln!("skip unreadable template: {}", path.display());
                    continue;
                };
                let template: Template = match serde_json::from_slice(&data) {
                    Ok(t) => t,
                    Err(e) => {
                        eprintln!("skip invalid template {}: {e}", path.display());
                        continue;
                    }
                };
                if Self::validate(&template).is_err() {
                    eprintln!("skip invalid template: {}", path.display());
                    continue;
                }
                if !template.id.starts_with("builtin-") && seen.insert(template.id.clone()) {
                    items.push(template)
                }
            }
        }
        Ok(items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_path_traversal_and_invalid_layout() {
        let store = TemplateStore {
            directory: std::env::temp_dir().join("unused-openink-test"),
        };
        let mut t = store.list().unwrap()[0].clone();
        t.id = "../escape".into();
        assert!(store.save(&t).is_err());
        t.id = "test".into();
        t.margin_mm = 1000.;
        assert!(TemplateStore::validate(&t).is_err());
        assert!(store.delete("../escape").is_err());
    }
    /// 样板工程的版心必须能在模板列表里找到，否则用户另存样板之后就再也
    /// 还原不出同一套版面。
    #[test]
    fn the_shipped_templates_back_the_starter_project() {
        let store = TemplateStore {
            directory: std::env::temp_dir().join("unused-openink-test"),
        };
        let items = store.list().unwrap();
        let physics = items.iter().find(|t| t.id == "builtin-physics").expect("builtin-physics");
        assert_eq!((physics.width_mm, physics.height_mm, physics.margin_mm, physics.lines), (210., 297., 20., 36));
        let grid = items.iter().find(|t| t.id == "builtin-grid").expect("builtin-grid");
        assert_eq!((grid.width_mm, grid.height_mm, grid.margin_mm, grid.lines), (190., 265., 19., 21));
    }
    /// 内置模板不能靠名字去重：用户目录里恰有一份同名文件时，两者都要在，
    /// 否则「另存为内置同名模板」会凭空消失。
    #[test]
    fn builtin_templates_survive_a_same_named_local_file() {
        let directory = std::env::temp_dir().join("openink-template-samename-test");
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("local.json"), r#"{"id":"physics-copy","name":"自物理实验报告","description":"","width_mm":210,"height_mm":297,"margin_mm":20,"font_size_mm":10,"letter_spacing_mm":1,"lines":36}"#).unwrap();
        let store = TemplateStore { directory };
        let items = store.list().unwrap();
        assert!(items.iter().any(|t| t.id == "builtin-physics"));
        assert!(items.iter().any(|t| t.id == "physics-copy"));
        assert_eq!(items.iter().filter(|t| t.name == "自物理实验报告").count(), 2);
        let _ = fs::remove_dir_all(store.directory);
    }
}
