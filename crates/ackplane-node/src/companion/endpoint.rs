use std::{io, path::Path};

use ackplane_client::companion::endpoint_name;
use interprocess::local_socket::{tokio::Listener, ListenerOptions};

pub(super) fn bind(directory: &Path) -> io::Result<Listener> {
    let metadata = std::fs::symlink_metadata(directory)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "node state must be a real directory",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
        if metadata.uid() != rustix::process::geteuid().as_raw() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "node state belongs to another user",
            ));
        }
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))?;
        let path = directory.join("ackplane-node.sock");
        match std::fs::symlink_metadata(&path) {
            Ok(metadata)
                if metadata.file_type().is_socket()
                    && metadata.uid() == rustix::process::geteuid().as_raw() =>
            {
                match std::os::unix::net::UnixStream::connect(&path) {
                    Ok(_) => {
                        return Err(io::Error::new(
                            io::ErrorKind::AddrInUse,
                            "node endpoint is already live",
                        ))
                    }
                    Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => {}
                    Err(error) => return Err(error),
                }
                std::fs::remove_file(path)?;
            }
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "node endpoint is not an owned socket",
                ))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    let options = ListenerOptions::new().name(endpoint_name(directory)?);
    #[cfg(windows)]
    let options = {
        use interprocess::os::windows::local_socket::ListenerOptionsExt;
        options.security_descriptor(super::windows::descriptor()?)
    };
    let listener = options.create_tokio()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            directory.join("ackplane-node.sock"),
            std::fs::Permissions::from_mode(0o600),
        )?;
    }
    Ok(listener)
}
