//package: basic
use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

use arcstr::literal;
use bytes::Bytes;
use futures::future::BoxFuture;
use num_bigint::BigUint;
use par_runtime::readback::Handle;
use par_runtime::{external_def, primitive::ParString};
use tokio::{
    fs::{self, DirEntry, File, OpenOptions},
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
};

external_def! {
    @basic/Os.{
        Path => path_from_bytes,
        Exists => os_exists,
        IsFile => os_is_file,
        IsDir => os_is_dir,
        Stdin => os_stdin,
        Stdout => os_stdout,
        Stderr => os_stderr,
        OpenFile => os_open_file,
        CreateOrReplaceFile => os_create_or_replace_file,
        CreateNewFile => os_create_new_file,
        AppendToFile => os_append_to_file,
        CreateOrAppendToFile => os_create_or_append_to_file,
        CreateDir => os_create_dir,
        RemoveFile => os_remove_file,
        RemoveDir => os_remove_dir,
        RemoveDirAll => os_remove_dir_all,
        CopyFile => os_copy_file,
        MoveFile => os_move_file,
        MoveDir => os_move_dir,
        ListDir => os_list_dir,
        TraverseDir => os_traverse_dir,
        Env => envmap_new,
    }
}

async fn path_from_bytes(mut handle: Handle) {
    let b = handle.receive().bytes().await;
    provide_path(handle, PathBuf::from(bytes_to_os(&b)));
}

fn provide_path(handle: Handle, path: PathBuf) {
    handle.provide_box(move |mut handle| {
        let path = path.clone();
        async move {
            match handle.case().await.as_str() {
                "name" => {
                    let bytes = path
                        .file_name()
                        .map(|n| os_to_bytes(n))
                        .unwrap_or_else(|| Bytes::new());
                    handle.provide_bytes(bytes);
                }
                "absolute" => {
                    let abs = absolute_path(&path);
                    let bytes = os_to_bytes(abs.as_os_str());
                    handle.provide_bytes(bytes);
                }
                "parts" => {
                    provide_bytes_parts(handle, &path);
                }
                "parent" => match path.parent() {
                    Some(p) => {
                        handle.signal(arcstr::literal!("some"));
                        provide_path(handle, p.to_path_buf());
                    }
                    None => {
                        handle.signal(arcstr::literal!("none"));
                        handle.break_();
                    }
                },
                "append" => {
                    let b = handle.receive().bytes().await;
                    provide_path(handle, path.join(bytes_to_os(&b)));
                }
                _ => unreachable!(),
            }
        }
    });
}

/// Makes a path absolute without touching the file system, so symlinks are not followed:
/// operations on a path act on the link itself, not on its target.
fn absolute_path(p: &Path) -> PathBuf {
    std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf())
}

fn provide_bytes_parts(mut handle: Handle, p: &Path) {
    for part in p.iter() {
        handle.signal(arcstr::literal!("item"));
        let bytes = os_to_bytes(part);
        handle.send().provide_bytes(bytes);
    }
    handle.signal(arcstr::literal!("end"));
    handle.break_();
}

#[cfg(unix)]
fn os_to_bytes(os: &OsStr) -> Bytes {
    use std::os::unix::ffi::OsStrExt;
    Bytes::copy_from_slice(os.as_bytes())
}

#[cfg(windows)]
fn os_to_bytes(os: &OsStr) -> Bytes {
    Bytes::copy_from_slice(os.as_encoded_bytes())
}

#[cfg(not(any(unix, windows)))]
fn os_to_bytes(os: &OsStr) -> Bytes {
    Bytes::from(os.to_string_lossy().as_ref())
}

#[cfg(unix)]
fn bytes_to_os(bytes: &[u8]) -> OsString {
    use std::os::unix::ffi::OsStrExt;
    OsStr::from_bytes(bytes).to_os_string()
}

/// Outside of Unix, OS strings aren't arbitrary bytes, so the bytes are read as UTF-8,
/// matching `os_to_bytes`.
#[cfg(not(unix))]
fn bytes_to_os(bytes: &[u8]) -> OsString {
    OsString::from(String::from_utf8_lossy(bytes).into_owned())
}

async fn provide_bytes_reader_from_async(mut handle: Handle, mut reader: impl AsyncRead + Unpin) {
    let mut buf = vec![0u8; 512];
    loop {
        match handle.case().await.as_str() {
            "close" | "#close" => {
                handle.signal(literal!("ok"));
                return handle.break_();
            }
            "read" => match reader.read(&mut buf[..]).await {
                Ok(n) => {
                    if n == 0 {
                        handle.signal(literal!("ok"));
                        handle.signal(literal!("end"));
                        return handle.break_();
                    }
                    handle.signal(literal!("ok"));
                    handle.signal(literal!("chunk"));
                    handle
                        .send()
                        .provide_bytes(Bytes::copy_from_slice(&buf[..n]));
                    continue;
                }
                Err(err) => {
                    handle.signal(literal!("err"));
                    return handle.provide_string(ParString::from(err.to_string()));
                }
            },
            _ => unreachable!(),
        }
    }
}

async fn provide_bytes_writer_from_async(mut handle: Handle, mut writer: impl AsyncWrite + Unpin) {
    loop {
        match handle.case().await.as_str() {
            "close" | "#close" => {
                // Try to flush pending data before closing
                match writer.flush().await {
                    Ok(()) => {
                        handle.signal(literal!("ok"));
                        return handle.break_();
                    }
                    Err(err) => {
                        handle.signal(literal!("err"));
                        return handle.provide_string(ParString::from(err.to_string()));
                    }
                }
            }
            "flush" => match writer.flush().await {
                Ok(()) => {
                    handle.signal(literal!("ok"));
                    continue;
                }
                Err(err) => {
                    handle.signal(literal!("err"));
                    return handle.provide_string(ParString::from(err.to_string()));
                }
            },
            "write" => {
                let bytes = handle.receive().bytes().await;
                match writer.write_all(bytes.as_ref()).await {
                    Ok(()) => {
                        handle.signal(literal!("ok"));
                        continue;
                    }
                    Err(err) => {
                        handle.signal(literal!("err"));
                        return handle.provide_string(ParString::from(err.to_string()));
                    }
                }
            }
            _ => unreachable!(),
        }
    }
}

fn provide_unit_io_result(mut handle: Handle, result: std::io::Result<()>) {
    match result {
        Ok(()) => {
            handle.signal(literal!("ok"));
            handle.break_();
        }
        Err(err) => {
            handle.signal(literal!("err"));
            handle.provide_string(ParString::from(err.to_string()));
        }
    }
}

fn provide_bool(mut handle: Handle, value: bool) {
    handle.signal(if value {
        literal!("true")
    } else {
        literal!("false")
    });
    handle.break_();
}

// The names of the entries of a directory, sorted by their byte representation.
async fn list_dir_names(dir: &Path) -> std::io::Result<Vec<OsString>> {
    let mut rd = fs::read_dir(dir).await?;
    let mut entries: Vec<(Bytes, OsString)> = Vec::new();
    while let Some(entry) = rd.next_entry().await? {
        let name = entry.file_name();
        entries.push((os_to_bytes(&name), name));
    }
    entries.sort_by(|(a, _), (b, _)| a.as_ref().cmp(b.as_ref()));
    Ok(entries.into_iter().map(|(_, name)| name).collect())
}

// Provide List<Os.Path> for the given entries of `base`.
fn provide_list_dir(mut handle: Handle, base: &Path, names: Vec<OsString>) {
    for name in names {
        let child = base.join(Path::new(&name));
        handle.signal(literal!("item"));
        provide_path(handle.send(), child);
    }
    handle.signal(literal!("end"));
    handle.break_();
}

// Directory tree node used for traverseDir
enum DirNode {
    File(PathBuf),
    Dir {
        path: PathBuf,
        children: Vec<DirNode>,
    },
}

// Recursively build the full directory tree. Returns an error message if any IO fails.
fn build_dir_tree(dir: PathBuf) -> BoxFuture<'static, Result<Vec<DirNode>, String>> {
    Box::pin(async move {
        let mut rd = fs::read_dir(&dir).await.map_err(|e| format!("{}", e))?;

        // Collect entries first to allow deterministic sorting
        let mut items: Vec<(Bytes, DirEntry)> = Vec::new();
        while let Some(entry) = rd.next_entry().await.map_err(|e| format!("{}", e))? {
            let name = entry.file_name();
            let key = os_to_bytes(&name);
            items.push((key, entry));
        }
        items.sort_by(|(a, _), (b, _)| a.as_ref().cmp(b.as_ref()));

        let mut result = Vec::new();
        for (_, entry) in items {
            let ty = entry.file_type().await.map_err(|e| format!("{}", e))?;
            let child_path = entry.path();
            if ty.is_dir() {
                let children = build_dir_tree(child_path.clone()).await?;
                result.push(DirNode::Dir {
                    path: child_path,
                    children,
                });
            } else {
                // Treat symlinks and others as files to avoid cycles
                result.push(DirNode::File(child_path));
            }
        }
        Ok(result)
    })
}

fn provide_dir_tree<'a>(mut handle: Handle, nodes: &'a [DirNode]) {
    match nodes.split_first() {
        None => {
            handle.signal(literal!("end"));
            handle.break_();
        }
        Some((node, tail)) => match node {
            DirNode::File(path) => {
                handle.signal(literal!("file"));
                provide_path(handle.send(), path.clone());
                provide_dir_tree(handle, tail);
            }
            DirNode::Dir { path, children } => {
                handle.signal(literal!("dir"));
                provide_path(handle.send(), path.clone());
                provide_dir_tree(handle.send(), children.as_slice());
                provide_dir_tree(handle, tail);
            }
        },
    }
}

async fn os_stdin(handle: Handle) {
    provide_bytes_reader_from_async(handle, tokio::io::stdin()).await;
}

async fn os_stdout(handle: Handle) {
    provide_bytes_writer_from_async(handle, tokio::io::stdout()).await;
}

async fn os_stderr(handle: Handle) {
    provide_bytes_writer_from_async(handle, tokio::io::stderr()).await;
}

async fn os_open_file(mut handle: Handle) {
    let path = pathbuf_from_os_path(handle.receive()).await;
    match File::open(&path).await {
        Ok(file) => {
            handle.signal(literal!("ok"));
            return provide_bytes_reader_from_async(handle, file).await;
        }
        Err(err) => {
            handle.signal(literal!("err"));
            return handle.provide_string(ParString::from(err.to_string()));
        }
    }
}

async fn os_create_or_replace_file(mut handle: Handle) {
    let path = pathbuf_from_os_path(handle.receive()).await;
    match OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&path)
        .await
    {
        Ok(file) => {
            handle.signal(literal!("ok"));
            return provide_bytes_writer_from_async(handle, file).await;
        }
        Err(err) => {
            handle.signal(literal!("err"));
            return handle.provide_string(ParString::from(err.to_string()));
        }
    }
}

async fn os_create_new_file(mut handle: Handle) {
    let path = pathbuf_from_os_path(handle.receive()).await;
    match OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)
        .await
    {
        Ok(file) => {
            handle.signal(literal!("ok"));
            return provide_bytes_writer_from_async(handle, file).await;
        }
        Err(err) => {
            handle.signal(literal!("err"));
            return handle.provide_string(ParString::from(err.to_string()));
        }
    }
}

async fn os_append_to_file(mut handle: Handle) {
    let path = pathbuf_from_os_path(handle.receive()).await;
    match OpenOptions::new()
        .write(true)
        .append(true)
        .open(&path)
        .await
    {
        Ok(file) => {
            handle.signal(literal!("ok"));
            return provide_bytes_writer_from_async(handle, file).await;
        }
        Err(err) => {
            handle.signal(literal!("err"));
            return handle.provide_string(ParString::from(err.to_string()));
        }
    }
}

async fn os_create_or_append_to_file(mut handle: Handle) {
    let path = pathbuf_from_os_path(handle.receive()).await;
    match OpenOptions::new()
        .create(true)
        .write(true)
        .append(true)
        .open(&path)
        .await
    {
        Ok(file) => {
            handle.signal(literal!("ok"));
            return provide_bytes_writer_from_async(handle, file).await;
        }
        Err(err) => {
            handle.signal(literal!("err"));
            return handle.provide_string(ParString::from(err.to_string()));
        }
    }
}

async fn os_create_dir(mut handle: Handle) {
    let path = pathbuf_from_os_path(handle.receive()).await;
    match fs::create_dir_all(&path).await {
        Ok(()) => {
            handle.signal(literal!("ok"));
            return handle.break_();
        }
        Err(err) => {
            handle.signal(literal!("err"));
            return handle.provide_string(ParString::from(err.to_string()));
        }
    }
}

async fn os_remove_file(mut handle: Handle) {
    let path = pathbuf_from_os_path(handle.receive()).await;
    provide_unit_io_result(handle, fs::remove_file(&path).await);
}

async fn os_remove_dir(mut handle: Handle) {
    let path = pathbuf_from_os_path(handle.receive()).await;
    provide_unit_io_result(handle, fs::remove_dir(&path).await);
}

async fn os_move_file(mut handle: Handle) {
    let src = pathbuf_from_os_path(handle.receive()).await;
    let dst = pathbuf_from_os_path(handle.receive()).await;
    provide_unit_io_result(handle, fs::rename(&src, &dst).await);
}

async fn os_move_dir(mut handle: Handle) {
    let src = pathbuf_from_os_path(handle.receive()).await;
    let dst = pathbuf_from_os_path(handle.receive()).await;
    provide_unit_io_result(handle, fs::rename(&src, &dst).await);
}

async fn os_remove_dir_all(mut handle: Handle) {
    let path = pathbuf_from_os_path(handle.receive()).await;
    provide_unit_io_result(handle, fs::remove_dir_all(&path).await);
}

async fn os_copy_file(mut handle: Handle) {
    let src = pathbuf_from_os_path(handle.receive()).await;
    let dst = pathbuf_from_os_path(handle.receive()).await;
    provide_unit_io_result(handle, fs::copy(&src, &dst).await.map(|_| ()));
}

async fn os_exists(mut handle: Handle) {
    let path = pathbuf_from_os_path(handle.receive()).await;
    provide_bool(handle, fs::try_exists(&path).await.unwrap_or(false));
}

async fn os_is_file(mut handle: Handle) {
    let path = pathbuf_from_os_path(handle.receive()).await;
    let metadata = fs::metadata(&path).await;
    provide_bool(handle, metadata.is_ok_and(|m| m.is_file()));
}

async fn os_is_dir(mut handle: Handle) {
    let path = pathbuf_from_os_path(handle.receive()).await;
    let metadata = fs::metadata(&path).await;
    provide_bool(handle, metadata.is_ok_and(|m| m.is_dir()));
}

async fn os_list_dir(mut handle: Handle) {
    let path = pathbuf_from_os_path(handle.receive()).await;
    match list_dir_names(&path).await {
        Ok(names) => {
            handle.signal(literal!("ok"));
            return provide_list_dir(handle, &path, names);
        }
        Err(err) => {
            handle.signal(literal!("err"));
            return handle.provide_string(ParString::from(err.to_string()));
        }
    }
}

async fn os_traverse_dir(mut handle: Handle) {
    let path = pathbuf_from_os_path(handle.receive()).await;
    match build_dir_tree(path.clone()).await {
        Ok(nodes) => {
            handle.signal(literal!("ok"));
            return provide_dir_tree(handle, nodes.as_slice());
        }
        Err(err) => {
            handle.signal(literal!("err"));
            return handle.provide_string(ParString::from(err));
        }
    }
}

async fn envmap_new(handle: Handle) {
    handle.provide_box(move |mut handle| async move {
        match handle.case().await.as_str() {
            "size" => {
                return handle.provide_nat(BigUint::from(std::env::vars_os().count()));
            }
            "keys" => {
                let vars: Vec<_> = std::env::vars_os().into_iter().collect();
                for (name, _) in vars {
                    handle.signal(literal!("item"));
                    handle.send().provide_bytes(os_to_bytes(&name));
                }
                handle.signal(literal!("end"));
                return handle.break_();
            }
            "list" => {
                let vars: Vec<_> = std::env::vars_os().into_iter().collect();
                for (name, value) in vars {
                    handle.signal(literal!("item"));
                    let mut pair = handle.send();
                    pair.send().provide_bytes(os_to_bytes(&name));
                    pair.provide_bytes(os_to_bytes(&value));
                }
                handle.signal(literal!("end"));
                return handle.break_();
            }
            "get" => {
                let name = handle.receive().bytes().await;
                match std::env::var_os(bytes_to_os(&name)) {
                    Some(val) => {
                        let bytes = os_to_bytes(&val);
                        handle.signal(literal!("some"));
                        return handle.provide_bytes(bytes);
                    }
                    None => {
                        handle.signal(literal!("none"));
                        return handle.break_();
                    }
                }
            }
            _ => unreachable!(),
        }
    });
}

async fn pathbuf_from_os_path(mut handle: Handle) -> PathBuf {
    handle.signal(literal!("absolute"));
    let path_bytes = handle.bytes().await;
    PathBuf::from(bytes_to_os(&path_bytes))
}

#[cfg(all(test, unix))]
mod tests {
    use super::absolute_path;

    #[test]
    fn absolute_paths_do_not_follow_symlinks() {
        let dir = std::env::temp_dir().join(format!("par-os-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("target");
        let link = dir.join("link");
        std::fs::write(&target, b"data").unwrap();
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let resolved = absolute_path(&link);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(resolved, link);
    }
}
