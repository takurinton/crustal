use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PostMeta {
    pub id: String,
    pub title: String,
    pub description: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Post {
    pub file_name: String,
    pub meta: PostMeta,
    pub body: String,
}

pub fn list_markdown_files(posts_dir: impl AsRef<Path>) -> io::Result<Vec<String>> {
    let mut files = Vec::new();

    for entry in fs::read_dir(posts_dir)? {
        let path = entry?.path();
        if !path.is_file() {
            continue;
        }

        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };

        if file_name.ends_with(".md") {
            files.push(file_name.to_string());
        }
    }

    Ok(files)
}

pub fn read_post(posts_dir: impl AsRef<Path>, md_file: &str) -> io::Result<Option<Post>> {
    let md = fs::read_to_string(posts_dir.as_ref().join(md_file))?;
    let frontmatter = crustal_markdown::get_frontmatter(&md);

    let Some(id) = frontmatter.get("id").map(ToString::to_string) else {
        return Ok(None);
    };
    let Some(title) = frontmatter.get("title").map(ToString::to_string) else {
        return Ok(None);
    };
    let Some(created_at) = frontmatter.get("created_at").map(ToString::to_string) else {
        return Ok(None);
    };

    let description = frontmatter
        .get("description")
        .map(ToString::to_string)
        .unwrap_or_default();
    let body = crustal_markdown::remove_frontmatter(&md);

    Ok(Some(Post {
        file_name: md_file.to_string(),
        meta: PostMeta {
            id,
            title,
            description,
            created_at,
        },
        body,
    }))
}

pub fn read_posts(posts_dir: impl AsRef<Path>) -> io::Result<Vec<Post>> {
    let posts_dir = posts_dir.as_ref();
    let mut posts = Vec::new();

    for md_file in list_markdown_files(posts_dir)? {
        if let Some(post) = read_post(posts_dir, &md_file)? {
            posts.push(post);
        }
    }

    sort_posts_desc(&mut posts);
    Ok(posts)
}

pub fn sort_posts_desc(posts: &mut [Post]) {
    posts.sort_by(|a, b| post_id_number(b).cmp(&post_id_number(a)));
}

pub fn copy_files_with_extension(
    src_dir: impl AsRef<Path>,
    dest_dir: impl AsRef<Path>,
    extension: &str,
) -> io::Result<usize> {
    let mut copied = 0;
    let dest_dir = dest_dir.as_ref();

    for entry in fs::read_dir(src_dir)? {
        let path = entry?.path();
        if !path.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some(extension) {
            continue;
        }

        let Some(file_name) = path.file_name() else {
            continue;
        };

        fs::create_dir_all(dest_dir)?;
        fs::copy(&path, dest_dir.join(file_name))?;
        copied += 1;
    }

    Ok(copied)
}

pub fn output_path(path: impl AsRef<Path>) -> PathBuf {
    Path::new("./").join(path)
}

fn post_id_number(post: &Post) -> i32 {
    post.meta.id.parse().unwrap_or_default()
}
