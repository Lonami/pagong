use crate::{Post, FOOTER_FILE_NAME, HEADER_FILE_NAME};

use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

// TODO we don't handle title and other metadata like tags
// TODO if we want to do this proper we should not put header inside main
const HTML_START: &str = r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8" />
</head>
<body>
    <main>
"#;

const HTML_END: &str = r#"    </main>
</body>
"#;

#[derive(Debug)]
pub struct Blog {
    pub posts: Vec<Post>,
    pub header: Option<String>,
    pub footer: Option<String>,
}

#[derive(Debug)]
pub enum FsAction {
    Copy {
        source: PathBuf,
        dest: PathBuf,
    },
    DeleteDir {
        path: PathBuf,
        not_exists_ok: bool,
        recursive: bool,
    },
    CreateDir {
        path: PathBuf,
        exists_ok: bool,
    },

    /// Creates file if it does not exist, overwrites if it does exist.
    WriteFile {
        path: PathBuf,
        content: String,
    },
}
use FsAction::*;

fn execute_fs_actions(actions: &[FsAction]) -> io::Result<()> {
    // This code is full of checks which are followed by actions, non-atomically.
    // This means that it's full of TOCTOU race conditions. I don't know how to avoid that.
    for action in actions {
        match action {
            Copy { source, dest } => {
                fs::copy(source, dest)?;
            }
            DeleteDir {
                path,
                not_exists_ok,
                recursive,
            } => {
                let should_fail_if_not_exists = !not_exists_ok;
                if should_fail_if_not_exists && !path.exists() {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        format!("There is nothing to delete at {}", path.to_string_lossy()),
                    ));
                }
                if *recursive {
                    fs::remove_dir_all(path)?;
                } else {
                    // Requires that the directory is empty
                    fs::remove_dir(path)?;
                }
            }
            CreateDir { path, exists_ok } => {
                if *exists_ok && path.exists() {
                    if !path.is_dir() {
                        return Err(io::Error::new(
                            io::ErrorKind::AlreadyExists,
                            format!(
                                "There is already a file (not a directory) at {}",
                                path.to_string_lossy()
                            ),
                        ));
                    }
                    return Ok(());
                }
                fs::create_dir(path)?;
            }
            WriteFile { path, content } => {
                if path.exists() && !path.is_file() {
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        format!(
                            "There is already a directory (not a file) at {}",
                            path.to_string_lossy()
                        ),
                    ));
                }

                // fs::write handles creation and truncation for us.
                fs::write(path, content)?;
            }
        }
    }

    Ok(())
}

impl Blog {
    pub fn from_source_dir<P: AsRef<Path>>(root: P) -> Result<Self, Box<dyn Error>> {
        let mut posts = vec![];
        let mut header = None;
        let mut footer = None;

        for child in fs::read_dir(root)? {
            let child = child?;
            let path = child.path();

            if let Some(name) = path.file_name() {
                if name == HEADER_FILE_NAME {
                    header = Some(fs::read_to_string(path)?);
                    continue;
                } else if name == FOOTER_FILE_NAME {
                    footer = Some(fs::read_to_string(path)?);
                    continue;
                }
            }

            posts.push(Post::from_source_file(path)?);
        }

        Ok(Self {
            posts,
            header,
            footer,
        })
    }

    pub fn generate<P: AsRef<Path>>(&self, root: P) -> io::Result<()> {
        let actions = self.generate_actions(root);
        execute_fs_actions(&actions)
    }

    pub fn generate_actions<P: AsRef<Path>>(&self, root: P) -> Vec<FsAction> {
        let mut actions = vec![];

        for post in self.posts.iter() {
            // TODO override name with metadata
            let post_file_stem = post.source.file_stem().expect("Post must have filename");
            let post_dir = root.as_ref().join(post_file_stem);
            actions.push(DeleteDir {
                path: post_dir.clone(),
                not_exists_ok: true,
                recursive: true,
            });
            actions.push(CreateDir {
                path: post_dir.clone(),
                exists_ok: false,
            });

            let post_path = post_dir.join("index.html");

            let mut file_content = String::new();
            file_content.push_str(HTML_START);
            file_content.push_str(&self.header.as_ref().unwrap_or(&String::new()));
            post.push_html(&mut file_content);
            file_content.push_str(&self.footer.as_ref().unwrap_or(&String::new()));
            file_content.push_str(HTML_END);

            actions.push(WriteFile {
                path: post_path,
                content: file_content,
            });

            for asset in post.assets.iter() {
                let asset_name = asset.file_name().expect("Asset must have file name");
                let dest_path = post_dir.join(asset_name);
                actions.push(Copy {
                    source: asset.into(),
                    dest: dest_path,
                });
            }
        }

        actions
    }
}

#[cfg(test)]
mod tests {
    // If FsAction stuff gets more complex, it might be worth implementing a mock
    // executor so that we can test for results rather than individual actions.
    use super::*;
    use chrono::offset::Local;

    #[test]
    fn standalone_file_post_generated() {
        let blog = Blog {
            posts: vec![Post {
                source: "/path/to/content/test_post.md".into(),
                markdown: "A test post".into(),
                title: "A test post title".into(),
                modified: Local::now(),
                created: Local::now(),
                assets: vec![],
            }],
            header: None,
            footer: None,
        };

        let actions = blog.generate_actions("dist");

        assert_eq!(actions.len(), 3);
        assert!(matches!(&actions[0] ,
           DeleteDir {
            path,
            not_exists_ok: true,
            recursive: true
           } if path == Path::new("dist/test_post")
        ));

        assert!(matches!(&actions[1] ,
           CreateDir {
            path,
            ..
           } if path == Path::new("dist/test_post")
        ));

        assert!(matches!(&actions[2] ,
           WriteFile {
            path,
            content
           } if path == Path::new("dist/test_post/index.html") && content.contains("A test post")
        ));
    }
}
