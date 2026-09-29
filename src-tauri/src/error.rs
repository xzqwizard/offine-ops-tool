use serde::Serialize;

/// 统一后端错误：序列化为 { kind, message } 传给前端
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Io(String),

    #[error("{0}")]
    Serialize(String),

    #[error("未找到: {0}")]
    NotFound(String),

    #[error("非法参数: {0}")]
    Invalid(String),
}

impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("AppError", 2)?;
        let kind = match self {
            AppError::Io(_) => "io",
            AppError::Serialize(_) => "serialize",
            AppError::NotFound(_) => "notFound",
            AppError::Invalid(_) => "invalid",
        };
        state.serialize_field("kind", kind)?;
        state.serialize_field("message", &self.to_string())?;
        state.end()
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::Io(e.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::Serialize(e.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
