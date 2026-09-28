//! Shader source text.
//!
//! The renderer reads the WGSL files from `assets/shaders/` at startup, so you can change a shader
//! without a new build. The same files are also built into the program. The renderer uses the
//! built-in copy if a file is missing or does not compile.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

/// One pass shader. `common.wgsl` is put in front of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShaderFile {
    World,
    Light,
    Composite,
    Sprite,
}

const COMMON: &str = include_str!("../../../assets/shaders/common.wgsl");

impl ShaderFile {
    fn file_name(self) -> &'static str {
        match self {
            ShaderFile::World => "world.wgsl",
            ShaderFile::Light => "light.wgsl",
            ShaderFile::Composite => "composite.wgsl",
            ShaderFile::Sprite => "sprite.wgsl",
        }
    }

    fn built_in(self) -> &'static str {
        match self {
            ShaderFile::World => include_str!("../../../assets/shaders/world.wgsl"),
            ShaderFile::Light => include_str!("../../../assets/shaders/light.wgsl"),
            ShaderFile::Composite => include_str!("../../../assets/shaders/composite.wgsl"),
            ShaderFile::Sprite => include_str!("../../../assets/shaders/sprite.wgsl"),
        }
    }

    /// The built-in source: common code plus the pass code.
    pub fn built_in_source(self) -> String {
        format!("{COMMON}\n{}", self.built_in())
    }

    /// The source from the shader folder, if both files can be read.
    pub fn source_from_dir(self, dir: &Path) -> Option<String> {
        let common = std::fs::read_to_string(dir.join("common.wgsl")).ok()?;
        let pass = std::fs::read_to_string(dir.join(self.file_name())).ok()?;
        Some(format!("{common}\n{pass}"))
    }
}

/// The default shader folder: `<assets>/shaders`.
pub fn default_shader_dir() -> PathBuf {
    foundry_content::default_assets_dir().join("shaders")
}

/// Make a shader module and a pipeline from it. First try the file in `dir`, then the built-in source.
/// `build` makes the pipeline from the module. Errors in the file version are logged, not fatal.
pub(crate) fn create_pipeline<T>(
    device: &wgpu::Device,
    file: ShaderFile,
    dir: Option<&Path>,
    build: impl Fn(&wgpu::ShaderModule) -> T,
) -> T {
    if let Some(source) = dir.and_then(|d| file.source_from_dir(d)) {
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(file.file_name()),
            source: wgpu::ShaderSource::Wgsl(Cow::Owned(source)),
        });
        let result = build(&module);
        match pollster::block_on(scope.pop()) {
            None => return result,
            Some(err) => log::error!(
                "shader {} from the assets folder has errors; using the built-in copy:\n{err}",
                file.file_name()
            ),
        }
    }
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(file.file_name()),
        source: wgpu::ShaderSource::Wgsl(Cow::Owned(file.built_in_source())),
    });
    build(&module)
}
