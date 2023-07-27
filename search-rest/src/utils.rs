use std::{io, iter};

pub fn read_certs(mut rd: impl io::BufRead) -> Result<Vec<Vec<u8>>, io::Error> {
    let certs = rustls_pemfile::certs(&mut rd)?;

    if certs.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "no certificates found",
        ));
    }

    Ok(certs)
}

pub fn read_cert(mut rd: impl io::BufRead) -> Result<Vec<u8>, io::Error> {
    for item in iter::from_fn(|| rustls_pemfile::read_one(&mut rd).transpose()) {
        let cert = match item? {
            rustls_pemfile::Item::X509Certificate(cert) => cert,
            _ => continue,
        };

        return Ok(cert);
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "no certificate found",
    ))
}

pub fn read_key(mut rd: impl io::BufRead) -> Result<Vec<u8>, io::Error> {
    for item in iter::from_fn(|| rustls_pemfile::read_one(&mut rd).transpose()) {
        let key = match item? {
            rustls_pemfile::Item::RSAKey(key)
            | rustls_pemfile::Item::PKCS8Key(key)
            | rustls_pemfile::Item::ECKey(key) => key,
            _ => continue,
        };

        return Ok(key);
    }

    Err(io::Error::new(io::ErrorKind::InvalidData, "no keys found"))
}
