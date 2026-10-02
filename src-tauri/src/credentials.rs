use crate::error::{AppError, AppResult};

// Windows DPAPI binds credentials to the current OS user. Passwords remain plaintext only
// in memory; portable exports strip office credentials and never export these ciphertexts.
pub fn protect(value: &str) -> AppResult<String> {
    if value.is_empty() || value.starts_with("dpapi:") {
        return Ok(value.into());
    }
    Ok(format!("dpapi:{}", transform(value, false)?))
}
pub fn reveal(value: &str) -> AppResult<String> {
    match value.strip_prefix("dpapi:") {
        Some(v) => transform(v, true),
        None => Ok(value.into()),
    }
}
#[cfg(windows)]
fn transform(value: &str, decrypt: bool) -> AppResult<String> {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
        },
    };
    let mut bytes = if decrypt {
        hex::decode(value).map_err(|_| AppError::Invalid("凭据密文无效".into()))?
    } else {
        value.as_bytes().to_vec()
    };
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_mut_ptr(),
    };
    let mut out = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    // DPAPI owns out.pbData until LocalFree. Input and output buffers remain valid for the call.
    let ok = unsafe {
        if decrypt {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
        } else {
            CryptProtectData(
                &input,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
        }
    };
    bytes.fill(0);
    if ok == 0 {
        return Err(AppError::Invalid(
            "系统凭据加解密失败，请在当前 Windows 用户下重新填写密码".into(),
        ));
    }
    let mut output =
        unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize) }.to_vec();
    unsafe {
        LocalFree(out.pbData.cast());
    }
    let result = if decrypt {
        String::from_utf8(output.clone()).map_err(|_| AppError::Invalid("凭据编码无效".into()))
    } else {
        Ok(hex::encode(&output))
    };
    output.fill(0);
    result
}
#[cfg(not(windows))]
fn transform(_value: &str, _decrypt: bool) -> AppResult<String> {
    Err(AppError::Invalid(
        "当前平台尚未配置系统凭据存储，不能保存明文办公凭据".into(),
    ))
}

pub fn secret_key(k: &str) -> bool {
    let k = k.to_ascii_uppercase();
    k.contains("PASSWORD")
        || k.contains("SECRET")
        || k.contains("TOKEN")
        || k.ends_with("_KEY")
        || k.ends_with("_PASS")
        || k.ends_with("_PWD")
}
pub fn secret_param(cat: &crate::catalog::CatalogFile, template: &str, key: &str) -> bool {
    secret_key(key)
        || cat
            .templates
            .iter()
            .find(|t| t.id == template)
            .is_some_and(|t| t.env_hints.iter().any(|e| e.key == key && e.secret))
}
pub fn protect_project(
    p: &mut crate::models::Project,
    cat: &crate::catalog::CatalogFile,
) -> AppResult<()> {
    if let Some(r) = &mut p.registry {
        r.password = protect(&r.password)?;
    }
    for i in &mut p.instances {
        for (k, v) in &mut i.params {
            if secret_param(cat, &i.template_id, k) {
                if let Some(s) = v.as_str() {
                    *v = serde_json::json!(protect(s)?);
                }
            }
        }
    }
    Ok(())
}
pub fn reveal_project(p: &mut crate::models::Project) -> AppResult<()> {
    if let Some(r) = &mut p.registry {
        r.password = reveal(&r.password)?;
    }
    for i in &mut p.instances {
        for v in i.params.values_mut() {
            if let Some(s) = v.as_str().filter(|s| s.starts_with("dpapi:")) {
                *v = serde_json::json!(reveal(s)?);
            }
        }
    }
    Ok(())
}
pub fn strip_project(p: &mut crate::models::Project, cat: &crate::catalog::CatalogFile) {
    if let Some(r) = &mut p.registry {
        r.password.clear();
    }
    for t in &mut p.template_snapshots {
        for h in &mut t.env_hints {
            if h.secret || secret_param(cat, &t.id, &h.key) {
                h.default.clear();
            }
        }
    }
    for i in &mut p.instances {
        i.params.retain(|k, v| {
            !secret_param(cat, &i.template_id, k)
                && !v.as_str().is_some_and(|v| v.starts_with("dpapi:"))
        });
    }
}
