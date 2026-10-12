use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use par_runtime::pkgid::PackageId;
use pathdiff::diff_paths;
use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};

use crate::model::{ItemKind, ModuleKey, PackageKind, SiteModel};

const FILE_SEGMENT: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'*')
    .add(b'/')
    .add(b':')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'\\')
    .add(b'|');

#[derive(Debug, Clone)]
pub struct SitePaths {
    pub index: PathBuf,
    pub css: PathBuf,
    pub js: PathBuf,
    pub package_pages: BTreeMap<PackageId, PathBuf>,
    pub module_pages: BTreeMap<ModuleKey, PathBuf>,
}

impl SitePaths {
    pub fn new(site: &SiteModel) -> Self {
        let mut package_pages = BTreeMap::new();
        let mut module_pages = BTreeMap::new();

        for package in site.packages.values() {
            let package_path = package_page_path(&package.id, package.kind);
            package_pages.insert(package.id.clone(), package_path.clone());

            for module in &package.modules {
                module_pages.insert(
                    module.key(),
                    package_path_for_module(&package_path, &module.path.to_slash_path()),
                );
            }
        }

        Self {
            index: PathBuf::from("index.html"),
            css: PathBuf::from("assets/site.css"),
            js: PathBuf::from("assets/site.js"),
            package_pages,
            module_pages,
        }
    }
}

pub fn item_anchor(kind: ItemKind, primary: &str) -> String {
    let prefix = match kind {
        ItemKind::Type => "type",
        ItemKind::Declaration => "dec",
    };
    format!("{prefix}-{}", encode_segment(primary))
}

pub fn relative_href(from_page: &Path, to_page: &Path) -> String {
    let base_dir = from_page.parent().unwrap_or_else(|| Path::new(""));
    let diff = diff_paths(to_page, base_dir).unwrap_or_else(|| to_page.to_path_buf());
    // File names may themselves be percent-encoded (like `nested%2FModule`), and browsers
    // decode links before looking files up, so each component is encoded once more.
    let text = diff
        .components()
        .map(|component| encode_segment(&component.as_os_str().to_string_lossy()))
        .collect::<Vec<_>>()
        .join("/");
    if text.is_empty() {
        String::from(".")
    } else {
        text
    }
}

pub fn relative_href_with_anchor(from_page: &Path, to_page: &Path, anchor: &str) -> String {
    if from_page == to_page {
        return format!("#{anchor}");
    }

    let mut href = relative_href(from_page, to_page);
    href.push('#');
    href.push_str(anchor);
    href
}

fn package_page_path(id: &PackageId, kind: PackageKind) -> PathBuf {
    let mut path = PathBuf::from("packages").join(kind_dir(kind));
    if kind != PackageKind::Root {
        path = path.join(encode_segment(id.name()));
    }
    path.join("index.html")
}

fn package_path_for_module(package_page: &Path, module_path: &str) -> PathBuf {
    package_page
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .join("modules")
        .join(encode_segment(module_path))
        .join("index.html")
}

fn kind_dir(kind: PackageKind) -> &'static str {
    match kind {
        PackageKind::Root => "root",
        PackageKind::BuiltIn => "builtin",
        PackageKind::DirectDependency => "dependency",
        PackageKind::IndirectDependency => "indirect",
    }
}

fn encode_segment(value: &str) -> String {
    utf8_percent_encode(value, FILE_SEGMENT).to_string()
}

#[cfg(test)]
mod tests {
    use super::relative_href;
    use std::path::Path;

    #[test]
    fn hrefs_to_percent_encoded_file_names_are_encoded_again() {
        let href = relative_href(
            Path::new("packages/root/index.html"),
            Path::new("packages/root/modules/nested%2FDeep/index.html"),
        );
        assert_eq!(href, "modules/nested%252FDeep/index.html");
    }
}
