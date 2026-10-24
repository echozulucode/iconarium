//! iconarium-cli: search, preview and export icons from the libraries indexed by the
//! Iconarium desktop app, from a terminal or scripts (`--json`), or as an MCP server for
//! LLM clients (`iconarium-cli mcp`).

mod logging;
mod mcp;
mod output;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};
use iconarium_catalog::{
    CatalogError, DetailOptions, ExportFormat, ExportNaming, ExportOptions, IconRef, IndexReader,
    PreviewOptions, SearchRequest, SheetBackground,
};

/// Exit code when a search finds nothing (so scripts can branch).
const EXIT_NO_RESULTS: u8 = 3;

#[derive(Parser)]
#[command(
    name = "iconarium-cli",
    version,
    about = "Search, preview and export icons from your Iconarium libraries. Run `iconarium-cli mcp` to serve them to LLM clients over MCP.",
    after_help = "Query syntax: words (all must match), \"exact phrase\", -exclude, globs (*.svg, network/*switch*), re:regex.\nThe index is the one the Iconarium app maintains; libraries appear here once opened in the app."
)]
struct Cli {
    /// Index file to read (default: the Iconarium app's index).
    #[arg(long, global = true, env = "ICONARIUM_INDEX", value_name = "FILE")]
    index: Option<PathBuf>,
    /// Print machine-readable JSON instead of text.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show where the index is and whether it can be read.
    Doctor,
    /// List indexed libraries.
    Libraries,
    /// Search icons by filename, path, tags, title, description or visible text.
    Search(SearchArgs),
    /// Show details for one icon.
    Show(ShowArgs),
    /// List tags with counts.
    Tags(TagsArgs),
    /// Browse a library's folders.
    Folders(FoldersArgs),
    /// Render icons into one labelled PNG contact sheet.
    Preview(PreviewArgs),
    /// Copy icons (SVG) or render them (PNG) into a folder.
    Export(ExportArgs),
    /// Add or remove tags (stored in each library's iconarium.yaml).
    Tag(TagArgs),
    /// Run the MCP server on stdio (for Claude Desktop, Claude Code, VS Code…).
    Mcp(mcp::McpArgs),
}

#[derive(Args)]
struct SearchArgs {
    /// Query (empty lists everything that passes the filters).
    query: Vec<String>,
    /// Library id, root path or name (repeatable; default: all).
    #[arg(long = "library", short = 'l', value_name = "LIB")]
    libraries: Vec<String>,
    /// Only this folder (path prefix inside the library).
    #[arg(long)]
    folder: Option<String>,
    /// Required tag (repeatable; all must match).
    #[arg(long = "tag", short = 't')]
    tags: Vec<String>,
    #[arg(long, default_value_t = 20)]
    limit: usize,
    /// Continue from a previous page's cursor.
    #[arg(long)]
    cursor: Option<String>,
}

#[derive(Args)]
struct ShowArgs {
    /// Icon id, or a path relative to the library root when --library is given.
    icon: String,
    /// Library for a path (id, root path or name).
    #[arg(long, short = 'l', value_name = "LIB")]
    library: Option<String>,
    /// Include the SVG markup.
    #[arg(long)]
    svg: bool,
    /// List byte-identical copies elsewhere in the index.
    #[arg(long)]
    duplicates: bool,
    #[arg(long, default_value_t = 200_000)]
    max_svg_bytes: usize,
}

#[derive(Args)]
struct TagsArgs {
    #[arg(long = "library", short = 'l', value_name = "LIB")]
    libraries: Vec<String>,
    /// Only tags starting with this.
    #[arg(long)]
    prefix: Option<String>,
    #[arg(long, default_value_t = 100)]
    limit: usize,
}

#[derive(Args)]
struct FoldersArgs {
    /// Folder inside the library (default: its root).
    folder: Option<String>,
    #[arg(long, short = 'l', value_name = "LIB")]
    library: Option<String>,
    #[arg(long, default_value_t = 50)]
    max_files: usize,
}

#[derive(Clone, Copy, ValueEnum)]
enum Bg {
    White,
    Checker,
    Transparent,
}

#[derive(Args)]
struct PreviewArgs {
    /// Icon ids (1–48).
    #[arg(required = true)]
    ids: Vec<u32>,
    /// Output PNG file.
    #[arg(long, short = 'o')]
    output: PathBuf,
    /// Cell size in px.
    #[arg(long, default_value_t = 128)]
    cell: u32,
    #[arg(long, value_enum, default_value_t = Bg::White)]
    background: Bg,
    #[arg(long)]
    no_labels: bool,
}

#[derive(Clone, Copy, ValueEnum)]
enum NameFrom {
    /// The file's own name.
    File,
    /// A slug of the icon's display name.
    Name,
}

#[derive(Args)]
struct ExportArgs {
    #[arg(required = true)]
    ids: Vec<u32>,
    /// Destination folder (created if needed).
    #[arg(long)]
    to: PathBuf,
    /// Render PNGs instead of copying SVGs.
    #[arg(long)]
    png: bool,
    /// PNG size (longest side, px).
    #[arg(long, default_value_t = 256)]
    size: u32,
    #[arg(long)]
    overwrite: bool,
    #[arg(long, value_enum, default_value_t = NameFrom::File)]
    name_from: NameFrom,
}

#[derive(Args)]
struct TagArgs {
    #[arg(required = true)]
    ids: Vec<u32>,
    /// Tag to add (repeatable).
    #[arg(long)]
    add: Vec<String>,
    /// Tag to remove (repeatable).
    #[arg(long)]
    remove: Vec<String>,
}

/// The index path from --index / ICONARIUM_INDEX, else the app's default location.
pub fn resolve_index(arg: Option<PathBuf>) -> Result<PathBuf, String> {
    arg.or_else(svg_core::paths::default_index_path)
        .ok_or_else(|| "cannot determine the Iconarium data folder; pass --index".to_string())
}

fn open(index: &std::path::Path) -> Result<IndexReader, CatalogError> {
    IndexReader::open(index).map_err(|e| match &e {
        CatalogError::Core(svg_core::CoreError::Io(io)) if io.kind() == std::io::ErrorKind::NotFound => {
            CatalogError::NotFound(format!(
                "no Iconarium index at {} — open a library in the Iconarium app first, or pass --index",
                index.display()
            ))
        }
        _ => e,
    })
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let index = match resolve_index(cli.index.clone()) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };
    if let Command::Mcp(args) = cli.command {
        logging::init(true);
        return mcp::run(index, args);
    }
    logging::init(false);
    match run(cli.command, &index, cli.json) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(cmd: Command, index: &std::path::Path, json: bool) -> Result<ExitCode, CatalogError> {
    if let Command::Doctor = cmd {
        return Ok(output::doctor(index, json));
    }
    let reader = open(index)?;
    match cmd {
        Command::Doctor | Command::Mcp(_) => unreachable!(),
        Command::Libraries => output::libraries(&reader.list_libraries(), json),
        Command::Search(a) => {
            let res = reader.search(&SearchRequest {
                query: a.query.join(" "),
                libraries: a.libraries,
                folder: a.folder,
                tags: a.tags,
                limit: Some(a.limit),
                cursor: a.cursor,
            })?;
            output::search(&res, json);
            if res.total == 0 {
                return Ok(ExitCode::from(EXIT_NO_RESULTS));
            }
        }
        Command::Show(a) => {
            let r = match a.library {
                Some(library) => IconRef::Path {
                    library,
                    path: a.icon,
                },
                None => IconRef::Id(a.icon.parse().map_err(|_| {
                    CatalogError::Invalid(format!(
                        "{:?} is not an icon id (use --library LIB for a path)",
                        a.icon
                    ))
                })?),
            };
            let d = reader.detail(
                &r,
                DetailOptions {
                    text: true,
                    svg: a.svg,
                    duplicates: a.duplicates,
                    max_svg_bytes: a.max_svg_bytes,
                },
            )?;
            output::detail(&d, json);
        }
        Command::Tags(a) => output::tags(
            &reader.list_tags(&a.libraries, a.prefix.as_deref(), a.limit)?,
            json,
        ),
        Command::Folders(a) => output::folders(
            &reader.list_folders(a.library.as_deref(), a.folder.as_deref(), a.max_files)?,
            json,
        ),
        Command::Preview(a) => {
            let p = reader.preview(
                &a.ids,
                PreviewOptions {
                    cell: a.cell,
                    background: match a.background {
                        Bg::White => SheetBackground::White,
                        Bg::Checker => SheetBackground::Checker,
                        Bg::Transparent => SheetBackground::Transparent,
                    },
                    labels: !a.no_labels,
                },
            )?;
            std::fs::write(&a.output, &p.png)?;
            output::preview(&p, &a.output, json);
        }
        Command::Export(a) => {
            let dest = std::path::absolute(&a.to)?;
            let res = reader.export(
                &a.ids,
                &ExportOptions {
                    dest,
                    format: if a.png {
                        ExportFormat::Png
                    } else {
                        ExportFormat::Svg
                    },
                    size: a.size,
                    overwrite: a.overwrite,
                    naming: match a.name_from {
                        NameFrom::File => ExportNaming::Original,
                        NameFrom::Name => ExportNaming::Name,
                    },
                },
                None,
            )?;
            output::export(&res, json);
            if res.written.is_empty() {
                return Ok(ExitCode::FAILURE);
            }
        }
        Command::Tag(a) => output::tagged(&reader.update_tags(&a.ids, &a.add, &a.remove)?, json),
    }
    Ok(ExitCode::SUCCESS)
}
