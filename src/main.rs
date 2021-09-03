use chrono::{NaiveDate, NaiveDateTime};
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::io;
use std::ops::Range;
use std::path::PathBuf;
use std::time::UNIX_EPOCH;

pub const SOURCE_PATH: &str = "content";
pub const TARGET_PATH: &str = "dist";
pub const DATE_FMT: &str = "%Y-%m-%d";

struct MdFile {
    path: PathBuf,
    title: String,
    date: NaiveDate,
    updated: NaiveDate,
    category: Option<String>,
    tags: Vec<String>,
    template: Option<PathBuf>,
    meta: HashMap<String, String>,
    md_offset: usize,
}

enum PreprocessorRule {
    Contents,
    Css,
    Toc { depth: u8 },
    Listing { path: PathBuf },
    Meta { key: String },
    Include { path: PathBuf },
}

struct Replacement {
    range: Range<usize>,
    rule: PreprocessorRule,
}

struct HtmlTemplate {
    path: Option<PathBuf>,
    replacements: Vec<Replacement>,
}

struct Scan {
    /// Root path of the source directory.
    source: PathBuf,
    /// Root path of the destination directory.
    destination: PathBuf,
    /// Directories to create in the destination.
    dirs_to_create: Vec<PathBuf>,
    /// Files to copy to the destination without any special treatment.
    files_to_copy: Vec<PathBuf>,
    /// CSS files found.
    css_files: Vec<PathBuf>,
    /// HTML templates found.
    html_templates: HashSet<HtmlTemplate>,
    /// Markdown files to parse and generate HTML from.
    md_files: Vec<MdFile>,
}

fn parse_opt_date(path: &PathBuf, created: bool, string: Option<&String>) -> NaiveDate {
    match string {
        Some(s) => match NaiveDate::parse_from_str(s, DATE_FMT) {
            Ok(d) => return d,
            Err(_) => eprintln!("note: invalid date value: {:?}", s),
        },
        None => {}
    }

    match fs::metadata(&path) {
        Ok(meta) => {
            if created {
                match meta.created() {
                    Ok(date) => {
                        return NaiveDateTime::from_timestamp(
                            date.duration_since(UNIX_EPOCH).unwrap().as_secs() as i64,
                            0,
                        )
                        .date()
                    }
                    Err(_) => eprintln!("note: failed to fetch creation date for file: {:?}", path),
                }
            } else {
                match meta.modified() {
                    Ok(date) => {
                        return NaiveDateTime::from_timestamp(
                            date.duration_since(UNIX_EPOCH).unwrap().as_secs() as i64,
                            0,
                        )
                        .date()
                    }
                    Err(_) => eprintln!(
                        "note: failed to fetch modification date for file: {:?}",
                        path
                    ),
                }
            }
        }
        Err(_) => eprintln!("note: failed to fetch metadata for file: {:?}", path),
    }

    chrono::Local::today().naive_local()
}

impl MdFile {
    pub fn new(root: &PathBuf, path: PathBuf) -> io::Result<Self> {
        let mut meta = HashMap::new();
        let mut md_offset = 0;

        let contents = fs::read_to_string(&path)?;

        if &contents[..4] == "+++\n" {
            if let Some(end_index) = contents.find("\n+++") {
                md_offset = end_index + 4;
                for line in contents[4..end_index].lines() {
                    if line.trim().is_empty() {
                        continue;
                    }
                    let index = match line.find('=') {
                        Some(i) => i,
                        None => {
                            eprintln!("note: metadata line without value: {:?}", line);
                            continue;
                        }
                    };
                    let key = line[..index].trim().to_owned();
                    let value = line[index + 1..].trim().to_owned();
                    meta.insert(key, value);
                }
            } else {
                eprintln!("note: md file with unclosed metadata: {:?}", path);
                md_offset = contents.len();
            }
        } else {
            eprintln!("note: md file without metadata: {:?}", path);
        }

        Ok(MdFile {
            title: match meta.get("title") {
                Some(s) => s.to_owned(),
                None => path.file_name().unwrap().to_str().unwrap().to_owned(),
            },
            date: parse_opt_date(&path, true, meta.get("date")),
            updated: parse_opt_date(&path, false, meta.get("updated")),
            category: meta.get("category").cloned(),
            tags: match meta.get("tags") {
                Some(s) => s.split(',').map(|s| s.trim().to_owned()).collect(),
                None => Vec::new(),
            },
            template: meta.get("template").map(|s| {
                if s.starts_with('/') {
                    let mut p = root.clone();
                    p.push(&s[1..]);
                    p
                } else {
                    let mut p = path.clone();
                    p.push(s);
                    p
                }
            }),
            path,
            meta,
            md_offset,
        })
    }
}

impl Scan {
    /// Creates a new scan on two stages. The first stage:
    ///
    /// * Detects all directories that need to be created.
    /// * Detects all CSS files.
    /// * Marks every file as needing a copy except for MD files.
    /// * Parses all MD files.
    ///
    /// The second stage:
    ///
    /// * Removes the HTML templates from the files that need copying.
    fn new(src: PathBuf, dst: PathBuf) -> io::Result<Self> {
        let mut scan = Scan {
            source: src,
            destination: dst,
            dirs_to_create: Vec::new(),
            files_to_copy: Vec::new(),
            css_files: Vec::new(),
            html_templates: HashSet::new(),
            md_files: Vec::new(),
        };
        let mut templates = HashSet::new();

        let mut pending = vec![scan.source.clone()];
        while let Some(src) = pending.pop() {
            for entry in fs::read_dir(&src)? {
                let entry = entry?;

                if entry.file_type()?.is_dir() {
                    pending.push(entry.path());
                    // Detects all directories that need to be created.
                    scan.dirs_to_create.push(entry.path());
                } else {
                    let filename = entry.file_name();
                    let filename = filename.to_str().expect("bad filename");
                    let ext_idx = filename
                        .rfind('.')
                        .map(|i| i + 1)
                        .unwrap_or_else(|| filename.len());
                    let ext = &filename[ext_idx..];

                    if ext.eq_ignore_ascii_case("css") {
                        // Detects all CSS files.
                        scan.css_files.push(entry.path());
                    }
                    if !ext.eq_ignore_ascii_case("md") {
                        // Marks every file as needing a copy except for MD files.
                        scan.files_to_copy.push(entry.path());
                    } else {
                        // Parses all MD files.
                        let md = MdFile::new(&scan.source, entry.path())?;
                        if let Some(template) = md.template.as_ref() {
                            templates.insert(template.clone());
                        }
                        scan.md_files.push(md);
                    }
                }
            }
        }

        // Removes the HTML templates from the files that need copying.
        scan.files_to_copy
            .retain(|path| templates.contains(path));

        Ok(scan)
    }

    /// Executes the scan:
    ///
    /// * Creates all directories that need creating.
    /// * Copies all files that need copying.
    /// * Converts every MD file to HTML and places it in the destination.
    fn execute(self) -> io::Result<()> {
        todo!()
    }
}

fn main() -> io::Result<()> {
    let root = match env::args().nth(1) {
        Some(path) => path.into(),
        None => env::current_dir()?,
    };

    let mut content = root.clone();
    content.push(SOURCE_PATH);

    let mut dist = root;
    dist.push(TARGET_PATH);

    Scan::new(content, dist)?.execute()?;

    Ok(())
}
