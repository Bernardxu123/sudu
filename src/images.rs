use crate::doc::resolve_image_path;
use iced::widget::{image as iced_image, svg as iced_svg};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub enum CachedImage {
    Raster {
        handle: iced_image::Handle,
        width: u32,
        height: u32,
    },
    Svg {
        handle: iced_svg::Handle,
        width: u32,
        height: u32,
    },
    Placeholder(String),
}

#[derive(Clone, Default)]
pub struct ImageStore {
    inner: Arc<Mutex<HashMap<String, CachedImage>>>,
}

impl ImageStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&self) {
        if let Ok(mut map) = self.inner.lock() {
            map.clear();
        }
    }

    pub fn get_or_load(&self, src: &str, doc_path: Option<&Path>, alt: &str) -> CachedImage {
        let resolved = resolve_image_path(src, doc_path);
        let key = resolved
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("remote:{src}"));
        if let Ok(map) = self.inner.lock() {
            if let Some(found) = map.get(&key) {
                return found.clone();
            }
        }
        let loaded = load_uncached(src, resolved.as_deref(), alt);
        if let Ok(mut map) = self.inner.lock() {
            if map.len() >= 256 {
                map.clear();
            }
            map.insert(key, loaded.clone());
        }
        loaded
    }
}

fn load_uncached(src: &str, path: Option<&Path>, alt: &str) -> CachedImage {
    let label = alt.trim();
    let fallback = |suffix: &str| {
        if label.is_empty() {
            CachedImage::Placeholder(format!("[图片{suffix}]"))
        } else {
            CachedImage::Placeholder(format!("[图片：{label}{suffix}]"))
        }
    };
    let Some(path) = path else {
        let lower = src.trim().to_ascii_lowercase();
        if lower.starts_with("http://") || lower.starts_with("https://") {
            return fallback("（网络图片）");
        }
        return fallback("");
    };
    let Ok(bytes) = std::fs::read(path) else {
        return fallback("（未找到）");
    };
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    if ext == "svg" {
        let (w, h) = parse_svg_dimensions(&bytes).unwrap_or((320, 200));
        return CachedImage::Svg {
            handle: iced_svg::Handle::from_memory(bytes),
            width: w.max(1),
            height: h.max(1),
        };
    }
    match image::load_from_memory(&bytes) {
        Ok(img) => {
            let rgba = img.to_rgba8();
            let (width, height) = rgba.dimensions();
            if width == 0 || height == 0 {
                return fallback("（空图）");
            }
            CachedImage::Raster {
                handle: iced_image::Handle::from_rgba(width, height, rgba.into_raw()),
                width,
                height,
            }
        }
        Err(_) => fallback("（格式不支持）"),
    }
}

fn parse_svg_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let head = std::str::from_utf8(&bytes[..bytes.len().min(2048)]).ok()?;
    let start = head.find("<svg")?;
    let end = head[start..].find('>')?;
    let tag = &head[start..start + end];
    let w = extract_svg_num(tag, "width");
    let h = extract_svg_num(tag, "height");
    if let (Some(w), Some(h)) = (w, h) {
        return Some((w.max(16), h.max(16)));
    }
    if let Some(vb) = extract_svg_attr(tag, "viewBox") {
        let nums: Vec<f32> = vb
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse::<f32>().ok())
            .collect();
        if nums.len() == 4 && nums[2] > 0.0 && nums[3] > 0.0 {
            return Some((nums[2].round() as u32, nums[3].round() as u32));
        }
    }
    None
}

fn extract_svg_attr<'a>(tag: &'a str, attr: &str) -> Option<&'a str> {
    let needle = format!("{attr}=");
    let pos = tag.find(&needle)?;
    let rest = tag[pos + needle.len()..].trim_start();
    let quote = rest.as_bytes().first().copied()?;
    if quote == b'"' || quote == b'\'' {
        let inner = &rest[1..];
        let end = inner.find(quote as char)?;
        Some(&inner[..end])
    } else {
        None
    }
}

fn extract_svg_num(tag: &str, attr: &str) -> Option<u32> {
    let val = extract_svg_attr(tag, attr)?;
    let digits: String = val
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    digits.parse::<f32>().ok().map(|v| v.round() as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn loads_local_png_and_handles_missing_and_remote() {
        let store = ImageStore::new();
        let doc_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("README.md");
        let loaded = store.get_or_load("assets/icon-256.png", Some(&doc_path), "图标");
        assert!(matches!(
            loaded,
            CachedImage::Raster { width: 256, height: 256, .. }
        ));
        let missing = store.get_or_load("assets/no-such-file.png", Some(&doc_path), "图");
        assert!(matches!(missing, CachedImage::Placeholder(s) if s.contains("未找到")));
        let remote = store.get_or_load("https://example.com/a.png", Some(&doc_path), "网图");
        assert!(matches!(remote, CachedImage::Placeholder(s) if s.contains("网络图片")));
    }
}
