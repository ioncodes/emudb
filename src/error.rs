use thiserror::Error;

#[derive(Debug, Error)]
#[allow(dead_code)]
pub enum JobError {
    #[error("authentication failed: {0}")]
    Auth(String),

    #[error("invalid request: {0}")]
    Validation(String),

    #[error("git/repo error: {0}")]
    Repo(String),

    #[error("gamelist/titlemap error: {0}")]
    GameSet(String),

    #[error("rom resolution error: {0}")]
    RomResolution(String),

    #[error("archive extraction error: {0}")]
    Archive(String),

    #[error("docker build failed: {0}")]
    DockerBuild(String),

    #[error("docker run failed: {0}")]
    DockerRun(String),

    #[error("post-processing failed: {0}")]
    PostProcess(String),

    #[error("output validation failed: {0}")]
    OutputValidation(String),

    #[error("upload failed: {0}")]
    Upload(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(String),
}

impl JobError {
    pub fn kind(&self) -> &'static str {
        match self {
            JobError::Auth(_) => "auth",
            JobError::Validation(_) => "validation",
            JobError::Repo(_) => "emulator_repo",
            JobError::GameSet(_) => "game_set",
            JobError::RomResolution(_) => "rom",
            JobError::Archive(_) => "archive",
            JobError::DockerBuild(_) => "build",
            JobError::DockerRun(_) => "run",
            JobError::PostProcess(_) => "postprocess",
            JobError::OutputValidation(_) => "output_validation",
            JobError::Upload(_) => "upload",
            JobError::Io(_) => "io",
            JobError::Other(_) => "internal",
        }
    }

    #[allow(dead_code)]
    pub fn status_code(&self) -> axum::http::StatusCode {
        use axum::http::StatusCode;
        match self {
            JobError::Auth(_) => StatusCode::UNAUTHORIZED,
            JobError::Validation(_) => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

pub type JobResult<T> = std::result::Result<T, JobError>;

impl From<anyhow::Error> for JobError {
    fn from(e: anyhow::Error) -> Self {
        JobError::Other(e.to_string())
    }
}
