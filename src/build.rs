use std::convert::TryInto;
use std::env;
use std::path::PathBuf;
use std::process::{self, Command};

use anyhow::{Context, Result};
use clap::Parser;
use fs_err as fs;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use xwin::util::ProgressTarget;

/// Compile a local package and all of its dependencies
#[derive(Clone, Debug, Default, Parser)]
#[clap(setting = clap::AppSettings::DeriveDisplayOrder, after_help = "Run `cargo help build` for more detailed information.")]
pub struct Build {
    /// Do not print cargo log messages
    #[clap(short = 'q', long)]
    pub quiet: bool,

    /// Package to build (see `cargo help pkgid`)
    #[clap(
        short = 'p',
        long = "package",
        value_name = "SPEC",
        multiple_values = true
    )]
    pub packages: Vec<String>,

    /// Build all packages in the workspace
    #[clap(long)]
    pub workspace: bool,

    /// Exclude packages from the build
    #[clap(long, value_name = "SPEC", multiple_values = true)]
    pub exclude: Vec<String>,

    /// Alias for workspace (deprecated)
    #[clap(long)]
    pub all: bool,

    /// Number of parallel jobs, defaults to # of CPUs
    #[clap(short = 'j', long, value_name = "N")]
    pub jobs: Option<usize>,

    /// Build only this package's library
    #[clap(long)]
    pub lib: bool,

    /// Build only the specified binary
    #[clap(long, value_name = "NAME", multiple_values = true)]
    pub bin: Vec<String>,

    /// Build all binaries
    #[clap(long)]
    pub bins: bool,

    /// Build only the specified example
    #[clap(long, value_name = "NAME", multiple_values = true)]
    pub example: Vec<String>,

    /// Build all examples
    #[clap(long)]
    pub examples: bool,

    /// Build only the specified test target
    #[clap(long, value_name = "NAME", multiple_values = true)]
    pub test: Vec<String>,

    /// Build all tests
    #[clap(long)]
    pub tests: bool,

    /// Build only the specified bench target
    #[clap(long, value_name = "NAME", multiple_values = true)]
    pub bench: Vec<String>,

    /// Build all benches
    #[clap(long)]
    pub benches: bool,

    /// Build all targets
    #[clap(long)]
    pub all_targets: bool,

    /// Build artifacts in release mode, with optimizations
    #[clap(short = 'r', long)]
    pub release: bool,

    /// Build artifacts with the specified Cargo profile
    #[clap(long, value_name = "PROFILE-NAME")]
    pub profile: Option<String>,

    /// Space or comma separated list of features to activate
    #[clap(long, multiple_values = true)]
    pub features: Vec<String>,

    /// Activate all available features
    #[clap(long)]
    pub all_features: bool,

    /// Do not activate the `default` feature
    #[clap(long)]
    pub no_default_features: bool,

    /// Build for the target triple
    #[clap(long, value_name = "TRIPLE", env = "CARGO_BUILD_TARGET")]
    pub target: Option<String>,

    /// Directory for all generated artifacts
    #[clap(long, value_name = "DIRECTORY", parse(from_os_str))]
    pub target_dir: Option<PathBuf>,

    /// Copy final artifacts to this directory (unstable)
    #[clap(long, value_name = "PATH", parse(from_os_str))]
    pub out_dir: Option<PathBuf>,

    /// Path to Cargo.toml
    #[clap(long, value_name = "PATH", parse(from_os_str))]
    pub manifest_path: Option<PathBuf>,

    /// Ignore `rust-version` specification in packages
    #[clap(long)]
    pub ignore_rust_version: bool,

    /// Error format
    #[clap(long, value_name = "FMT", multiple_values = true)]
    pub message_format: Vec<String>,

    /// Output the build plan in JSON (unstable)
    #[clap(long)]
    pub build_plan: bool,

    /// Output build graph in JSON (unstable)
    #[clap(long)]
    pub unit_graph: bool,

    /// Outputs a future incompatibility report at the end of the build (unstable)
    #[clap(long)]
    pub future_incompat_report: bool,

    /// Use verbose output (-vv very verbose/build.rs output)
    #[clap(short = 'v', long, parse(from_occurrences), max_occurrences = 2)]
    pub verbose: usize,

    /// Coloring: auto, always, never
    #[clap(long, value_name = "WHEN")]
    pub color: Option<String>,

    /// Require Cargo.lock and cache are up to date
    #[clap(long)]
    pub frozen: bool,

    /// Require Cargo.lock is up to date
    #[clap(long)]
    pub locked: bool,

    /// Run without accessing the network
    #[clap(long)]
    pub offline: bool,

    /// Override a configuration value (unstable)
    #[clap(long, value_name = "KEY=VALUE", multiple_values = true)]
    pub config: Vec<String>,

    /// Unstable (nightly-only) flags to Cargo, see 'cargo -Z help' for details
    #[clap(short = 'Z', value_name = "FLAG", multiple_values = true)]
    pub unstable_flags: Vec<String>,

    /// xwin cache directory
    #[clap(long, parse(from_os_str), env = "XWIN_CACHE_DIR", hide = true)]
    pub xwin_cache_dir: Option<PathBuf>,

    /// The architectures to include in CRT/SDK
    #[clap(
        long,
        env = "XWIN_ARCH",
        possible_values(&["x86", "x86_64", "aarch", "aarch64"]),
        use_value_delimiter = true,
        default_value = "x86_64,aarch64",
        hide = true,
    )]
    pub xwin_arch: Vec<xwin::Arch>,

    /// The variants to include
    #[clap(
        long,
        env = "XWIN_VARIANT",
        possible_values(&["desktop", "onecore", /*"store",*/ "spectre"]),
        use_value_delimiter = true,
        default_value = "desktop",
        hide = true,
    )]
    pub xwin_variant: Vec<xwin::Variant>,

    /// The version to retrieve, can either be a major version of 15 or 16, or
    /// a "<major>.<minor>" version.
    #[clap(long, env = "XWIN_VERSION", default_value = "16", hide = true)]
    pub xwin_version: String,
}

impl Build {
    /// Execute `cargo build` command
    pub fn execute(&self) -> Result<()> {
        let mut build = self.build_command("build")?;
        let mut child = build.spawn().context("Failed to run cargo build")?;
        let status = child.wait().expect("Failed to wait on cargo build process");
        if !status.success() {
            process::exit(status.code().unwrap_or(1));
        }
        Ok(())
    }

    /// Generate cargo subcommand
    pub fn build_command(&self, subcommand: &str) -> Result<Command> {
        let xwin_cache_dir = self.xwin_cache_dir.clone().unwrap_or_else(|| {
            dirs::cache_dir()
                // If the really is no cache dir, cwd will also do
                .unwrap_or_else(|| env::current_dir().expect("Failed to get current dir"))
                .join(env!("CARGO_PKG_NAME"))
                .join("xwin")
        });
        fs::create_dir_all(&xwin_cache_dir)?;

        let mut build = Command::new("cargo");
        build.arg(subcommand);

        // collect cargo build arguments
        if self.quiet {
            build.arg("--quiet");
        }
        for pkg in &self.packages {
            build.arg("--package").arg(pkg);
        }
        if self.workspace {
            build.arg("--workspace");
        }
        for item in &self.exclude {
            build.arg("--excude").arg(item);
        }
        if self.all {
            build.arg("--all");
        }
        if let Some(jobs) = self.jobs {
            build.arg("--jobs").arg(jobs.to_string());
        }
        if self.lib {
            build.arg("--lib");
        }
        for bin in &self.bin {
            build.arg("--bin").arg(bin);
        }
        if self.bins {
            build.arg("--bins");
        }
        for example in &self.example {
            build.arg("--example").arg(example);
        }
        if self.examples {
            build.arg("--examples");
        }
        for test in &self.test {
            build.arg("--test").arg(test);
        }
        if self.tests {
            build.arg("--tests");
        }
        for bench in &self.bench {
            build.arg("--bench").arg(bench);
        }
        if self.benches {
            build.arg("--benches");
        }
        if self.all_targets {
            build.arg("--all-targets");
        }
        if self.release {
            build.arg("--release");
        }
        if let Some(profile) = self.profile.as_ref() {
            build.arg("--profile").arg(profile);
        }
        for feature in &self.features {
            build.arg("--features").arg(feature);
        }
        if self.all_features {
            build.arg("--all-features");
        }
        if self.no_default_features {
            build.arg("--no-default-features");
        }
        if let Some(target) = self.target.as_ref() {
            build.arg("--target").arg(target);
        }
        if let Some(dir) = self.target_dir.as_ref() {
            build.arg("--target-dir").arg(dir);
        }
        if let Some(dir) = self.out_dir.as_ref() {
            build.arg("--out-dir").arg(dir);
        }
        if let Some(path) = self.manifest_path.as_ref() {
            build.arg("--manifest-path").arg(path);
        }
        if self.ignore_rust_version {
            build.arg("--ignore-rust-version");
        }
        for fmt in &self.message_format {
            build.arg("--message-format").arg(fmt);
        }
        if self.build_plan {
            build.arg("--build-plan");
        }
        if self.unit_graph {
            build.arg("--unit-graph");
        }
        if self.future_incompat_report {
            build.arg("--future-incompat-report");
        }
        if self.verbose > 0 {
            build.arg(format!("-{}", "v".repeat(self.verbose)));
        }
        if let Some(color) = self.color.as_ref() {
            build.arg("--color").arg(color);
        }
        if self.frozen {
            build.arg("--frozen");
        }
        if self.locked {
            build.arg("--locked");
        }
        if self.offline {
            build.arg("--offline");
        }
        for config in &self.config {
            build.arg("--config").arg(config);
        }
        for flag in &self.unstable_flags {
            build.arg("-Z").arg(flag);
        }

        if let Some(target) = self.target.as_ref() {
            if target.contains("msvc") {
                self.setup_msvc_crt(xwin_cache_dir.clone())?;
                let env_target = target.to_uppercase().replace('-', "_");
                build.env("TARGET_CC", format!("clang-cl --target={}", target));
                build.env("TARGET_CXX", format!("clang-cl --target={}", target));
                build.env(
                    format!("CC_{}", env_target.to_lowercase()),
                    format!("clang-cl --target={}", target),
                );
                build.env(
                    format!("CXX_{}", env_target.to_lowercase()),
                    format!("clang-cl --target={}", target),
                );
                build.env("TARGET_AR", "llvm-lib");
                build.env(format!("AR_{}", env_target), "llvm-lib");
                build.env(format!("CARGO_TARGET_{}_LINKER", env_target), "lld-link");

                let cl_flags = format!(
                    "-fuse-ld=lld-link /imsvc{dir}/crt/include /imsvc{dir}/sdk/include/ucrt /imsvc{dir}/sdk/include/um /imsvc{dir}/sdk/include/shared",
                    dir = xwin_cache_dir.display()
                );
                build.env("CL_FLAGS", &cl_flags);
                build.env(format!("CFLAGS_{}", env_target.to_lowercase()), &cl_flags);
                build.env(format!("CXXFLAGS_{}", env_target.to_lowercase()), &cl_flags);

                let target_arch = target
                    .split_once('-')
                    .map(|(x, _)| x)
                    .context("invalid target triple")?;
                let rustflags = format!(
                    "-Lnative={dir}/crt/lib/{arch} -Lnative={dir}/sdk/lib/um/{arch} -Lnative={dir}/sdk/lib/ucrt/{arch}",
                    dir = xwin_cache_dir.display(),
                    arch = target_arch,
                );
                build.env(format!("CARGO_TARGET_{}_RUSTFLAGS", env_target), rustflags);

                #[cfg(target_os = "macos")]
                if let Ok(path) = env::var("PATH") {
                    let mut new_path = path.clone();
                    if cfg!(target_arch = "x86_64") && !path.contains("/usr/local/opt/llvm/bin") {
                        new_path.push_str(":/usr/local/opt/llvm/bin");
                    } else if cfg!(target_arch = "aarch64")
                        && !path.contains("/opt/homebrew/opt/llvm/bin")
                    {
                        new_path.push_str(":/opt/homebrew/opt/llvm/bin");
                    }
                    build.env("PATH", new_path);
                }
            }
        }

        Ok(build)
    }

    fn setup_msvc_crt(&self, cache_dir: PathBuf) -> Result<()> {
        let done_mark_file = cache_dir.join("DONE");
        if done_mark_file.is_file() {
            return Ok(());
        }

        let draw_target = ProgressTarget::Stdout;
        let ctx = if self.xwin_cache_dir.is_some() {
            xwin::Ctx::with_dir(cache_dir.clone().try_into()?, draw_target)?
        } else {
            xwin::Ctx::with_temp(draw_target)?
        };
        let ctx = std::sync::Arc::new(ctx);
        let pkg_manifest = self.load_manifest(&ctx, draw_target)?;

        let arches = self
            .xwin_arch
            .iter()
            .fold(0, |acc, arch| acc | *arch as u32);
        let variants = self
            .xwin_variant
            .iter()
            .fold(0, |acc, var| acc | *var as u32);
        let pruned = xwin::prune_pkg_list(&pkg_manifest, arches, variants)?;
        let op = xwin::Ops::Splat(xwin::SplatConfig {
            include_debug_libs: false,
            include_debug_symbols: false,
            enable_symlinks: !cfg!(target_os = "macos"),
            preserve_ms_arch_notation: false,
            copy: false,
            output: cache_dir.clone().try_into()?,
        });
        let pkgs = pkg_manifest.packages;

        let mp = MultiProgress::with_draw_target(draw_target.into());
        let work_items: Vec<_> = pruned
        .into_iter()
        .map(|pay| {
            let prefix = match pay.kind {
                xwin::PayloadKind::CrtHeaders => "CRT.headers".to_owned(),
                xwin::PayloadKind::CrtLibs => {
                    format!(
                        "CRT.libs.{}.{}",
                        pay.target_arch.map(|ta| ta.as_str()).unwrap_or("all"),
                        pay.variant.map(|v| v.as_str()).unwrap_or("none")
                    )
                }
                xwin::PayloadKind::SdkHeaders => {
                    format!(
                        "SDK.headers.{}.{}",
                        pay.target_arch.map(|v| v.as_str()).unwrap_or("all"),
                        pay.variant.map(|v| v.as_str()).unwrap_or("none")
                    )
                }
                xwin::PayloadKind::SdkLibs => {
                    format!(
                        "SDK.libs.{}",
                        pay.target_arch.map(|ta| ta.as_str()).unwrap_or("all")
                    )
                }
                xwin::PayloadKind::SdkStoreLibs => "SDK.libs.store.all".to_owned(),
                xwin::PayloadKind::Ucrt => "SDK.ucrt.all".to_owned(),
            };

            let pb = mp.add(
                ProgressBar::with_draw_target(0, draw_target.into()).with_prefix(prefix).with_style(
                    ProgressStyle::default_bar()
                        .template("{spinner:.green} {prefix:.bold} [{elapsed}] {wide_bar:.green} {bytes}/{total_bytes} {msg}").unwrap()
                        .progress_chars("=> "),
                ),
            );
            xwin::WorkItem {
                payload: std::sync::Arc::new(pay),
                progress: pb,
            }
        })
        .collect();

        mp.set_move_cursor(true);
        ctx.execute(pkgs, work_items, arches, variants, op)?;
        fs::write(done_mark_file, "")?;

        let dl = cache_dir.join("dl");
        if dl.exists() {
            let _ = fs::remove_dir_all(dl);
        }
        let unpack = cache_dir.join("unpack");
        if unpack.exists() {
            let _ = fs::remove_dir_all(unpack);
        }
        Ok(())
    }

    fn load_manifest(
        &self,
        ctx: &xwin::Ctx,
        dt: ProgressTarget,
    ) -> Result<xwin::manifest::PackageManifest> {
        let manifest_pb = ProgressBar::with_draw_target(0, dt.into())
            .with_style(
            ProgressStyle::default_bar()
                .template(
                    "{spinner:.green} {prefix:.bold} [{elapsed}] {wide_bar:.green} {bytes}/{total_bytes} {msg}",
                )?
                .progress_chars("=> "),
        );
        manifest_pb.set_prefix("Manifest");
        manifest_pb.set_message("📥 downloading");

        let manifest =
            xwin::manifest::get_manifest(ctx, &self.xwin_version, "release", manifest_pb.clone())?;
        let pkg_manifest =
            xwin::manifest::get_package_manifest(ctx, &manifest, manifest_pb.clone())?;
        manifest_pb.finish_with_message("📥 downloaded");
        Ok(pkg_manifest)
    }
}
