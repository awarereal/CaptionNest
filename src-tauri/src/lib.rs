//! CaptionNest desktop app: Tauri commands and app setup.

mod asr;
#[cfg(feature = "automation")]
mod automation;
mod captions;
mod error;
mod fonts;
mod jobs;
mod media;
mod models;
mod paths;
mod pipeline;
mod project;
mod selftest;
mod settings;
mod translate;
mod voice;

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use captions::{validate, Cue};
use error::{AppError, AppResult, ErrorCode};
use jobs::{JobKind, Jobs, Reporter};
use models::catalog;
use paths::Paths;

pub struct AppState {
    paths: Paths,
    jobs: Jobs,
    settings: Mutex<settings::Settings>,
    http: reqwest::Client,
    /// The Deepgram key for voice typing (never sent to the web view).
    voice: voice::VoiceKey,
    self_test: bool,
}

type Cmd<T> = Result<T, AppError>;

// ---------- app ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppInfo {
    version: String,
    os: &'static str,
    arch: &'static str,
    gpu_backend: Option<&'static str>,
    models_dir: String,
    log_dir: Option<String>,
}

#[tauri::command]
fn app_info(app: AppHandle, state: State<AppState>) -> AppInfo {
    let gpu_backend = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("Metal")
    } else if cfg!(windows) {
        Some("Vulkan")
    } else {
        None
    };
    AppInfo {
        version: app.package_info().version.to_string(),
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        gpu_backend,
        models_dir: state.paths.models.to_string_lossy().into_owned(),
        log_dir: app.path().app_log_dir().ok().map(|p| p.to_string_lossy().into_owned()),
    }
}

/// The window starts hidden and is shown once the UI has painted (no white flash).
#[tauri::command]
fn show_main_window(app: AppHandle, state: State<AppState>) {
    if state.self_test {
        return;
    }
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

#[tauri::command]
fn get_settings(state: State<AppState>) -> settings::Settings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
fn save_settings(state: State<AppState>, settings: settings::Settings) -> Cmd<()> {
    settings::save(&state.paths.config, &settings)?;
    *state.settings.lock().unwrap() = settings;
    Ok(())
}

// ---------- media ----------

/// Lets the web view play a local file through the asset protocol.
fn allow_media(app: &AppHandle, path: &Path) {
    if let Err(e) = app.asset_protocol_scope().allow_file(path) {
        log::warn!("asset scope: {e}");
    }
}

#[tauri::command]
async fn probe_media(app: AppHandle, state: State<'_, AppState>, path: String) -> Cmd<media::probe::MediaInfo> {
    let p = PathBuf::from(&path);
    let info = pipeline::probe(&state.paths, &p).await?;
    allow_media(&app, &p);
    Ok(info)
}

#[tauri::command]
async fn start_preview(app: AppHandle, state: State<'_, AppState>, path: String, audio_stream_index: Option<u32>) -> Cmd<String> {
    let (job_id, cancel) = state.jobs.begin(JobKind::Preview);
    let id = job_id.clone();
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = handle.state::<AppState>();
        let report = Reporter::new(handle.clone(), id.clone(), JobKind::Preview);
        let result = jobs::guard(async {
            report.stage("converting", Some(0.0), None);
            let info = pipeline::probe(&state.paths, Path::new(&path)).await?;
            let out = media::preview::cache_path(&state.paths.previews, &info, audio_stream_index);
            let preview = media::preview::create_preview(&state.paths.tool("ffmpeg")?, &info, audio_stream_index, &out, &cancel, |p| {
                report.stage("converting", Some(p), None)
            })
            .await?;
            allow_media(&handle, Path::new(&preview.path));
            Ok::<_, AppError>(preview)
        })
        .await;
        jobs::finish(&handle, &state.jobs, JobKind::Preview, &id, result);
    });
    Ok(job_id)
}

// ---------- video with burned-in captions ----------

/// A folder for the caption images of one video export. Returns its token.
#[tauri::command]
fn burn_begin(state: State<'_, AppState>) -> Cmd<String> {
    let id = uuid::Uuid::new_v4().to_string();
    std::fs::create_dir_all(state.paths.jobs.join(format!("burn-{id}")))?;
    Ok(id)
}

/// Saves one caption image (PNG bytes as the raw request body).
#[tauri::command]
fn burn_frame(state: State<'_, AppState>, request: tauri::ipc::Request<'_>) -> Cmd<()> {
    let header = |name: &str| request.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_string);
    let (Some(id), Some(file)) = (header("x-burn-id"), header("x-file")) else {
        return Err(AppError::internal("caption image without a name"));
    };
    if uuid::Uuid::parse_str(&id).is_err() || !media::burn::valid_frame_name(&file) {
        return Err(AppError::internal("invalid caption image name"));
    }
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err(AppError::internal("caption image must be sent as bytes"));
    };
    let dir = state.paths.jobs.join(format!("burn-{id}"));
    if !dir.is_dir() {
        return Err(AppError::internal("caption image folder is gone"));
    }
    std::fs::write(dir.join(file), bytes)?;
    Ok(())
}

#[tauri::command]
async fn start_burn(app: AppHandle, state: State<'_, AppState>, request: media::burn::BurnRequest) -> Cmd<String> {
    if uuid::Uuid::parse_str(&request.frames_id).is_err() || !media::burn::valid_frame_name(&request.blank) {
        return Err(AppError::internal("invalid caption image folder"));
    }
    let (job_id, cancel) = state.jobs.begin(JobKind::Burn);
    let id = job_id.clone();
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = handle.state::<AppState>();
        let report = Reporter::new(handle.clone(), id.clone(), JobKind::Burn);
        // The images are removed when the export ends, however it ends.
        let frames = jobs::ScratchDir::create(&state.paths.jobs, &format!("burn-{}", request.frames_id));
        let result = jobs::guard(async {
            let frames = frames?;
            report.stage("rendering", Some(0.0), None);
            let info = pipeline::probe(&state.paths, Path::new(&request.video_path)).await?;
            media::burn::burn(&state.paths.tool("ffmpeg")?, &info, &request, frames.path(), &cancel, |p| report.stage("rendering", Some(p), None)).await
        })
        .await;
        jobs::finish(&handle, &state.jobs, JobKind::Burn, &id, result);
    });
    Ok(job_id)
}

/// Checks a Sarvam API key before it's saved.
#[tauri::command]
async fn check_online_key(state: State<'_, AppState>, key: String) -> Cmd<()> {
    let key = key.trim().to_string();
    if key.is_empty() {
        return Err(AppError::new(ErrorCode::OnlineKey, "Paste your Sarvam API key first."));
    }
    asr::sarvam::check_key(&asr::sarvam::Online { key, http: state.http.clone() }).await
}

// ---------- caption fonts the user adds ----------

#[tauri::command]
fn list_fonts(state: State<AppState>) -> Vec<fonts::UserFont> {
    fonts::list(&state.paths.fonts)
}

#[tauri::command]
async fn import_fonts(state: State<'_, AppState>, paths: Vec<String>) -> Cmd<fonts::ImportResult> {
    let dir = state.paths.fonts.clone();
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    tauri::async_runtime::spawn_blocking(move || fonts::import(&dir, &paths))
        .await
        .map_err(|e| AppError::internal(e.to_string()))?
}

/// One added font file as raw bytes (the web view builds a FontFace from them).
#[tauri::command]
async fn font_file(state: State<'_, AppState>, file: String) -> Cmd<tauri::ipc::Response> {
    let dir = state.paths.fonts.clone();
    let bytes = tauri::async_runtime::spawn_blocking(move || fonts::read_file(&dir, &file))
        .await
        .map_err(|e| AppError::internal(e.to_string()))??;
    Ok(tauri::ipc::Response::new(bytes))
}

#[tauri::command]
fn remove_font(state: State<AppState>, id: String) -> Cmd<Vec<fonts::UserFont>> {
    fonts::remove(&state.paths.fonts, &id)
}

// ---------- voice typing ----------

#[tauri::command]
fn voice_status(state: State<AppState>) -> voice::Status {
    state.voice.status()
}

/// Checks a Deepgram key with Deepgram, then saves it.
#[tauri::command]
async fn save_voice_key(state: State<'_, AppState>, key: String) -> Cmd<voice::Status> {
    let key = key.trim().to_string();
    if key.is_empty() {
        return Err(AppError::new(ErrorCode::VoiceKey, "Paste your Deepgram API key first."));
    }
    voice::check_key(&state.http, &key).await?;
    state.voice.set(Some(key))?;
    Ok(state.voice.status())
}

#[tauri::command]
fn remove_voice_key(state: State<AppState>) -> Cmd<voice::Status> {
    state.voice.set(None)?;
    Ok(state.voice.status())
}

/// Turns one microphone recording (raw bytes; `x-mime` and `x-language` headers) into text.
#[tauri::command]
async fn transcribe_voice(state: State<'_, AppState>, request: tauri::ipc::Request<'_>) -> Cmd<voice::Heard> {
    let header = |name: &str| request.headers().get(name).and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
    let (mime, language) = (header("x-mime"), header("x-language"));
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err(AppError::internal("the recording must be sent as bytes"));
    };
    let Some(key) = state.voice.get() else {
        return Err(AppError::new(ErrorCode::VoiceKey, "Add your Deepgram API key in Settings → Voice typing to use voice typing."));
    };
    voice::transcribe(&state.http, &key, bytes.clone(), &mime, &language).await
}

// ---------- generation ----------

#[tauri::command]
async fn start_generate(app: AppHandle, state: State<'_, AppState>, request: pipeline::GenerateRequest) -> Cmd<String> {
    let (job_id, cancel) = state.jobs.begin(JobKind::Generate);
    let id = job_id.clone();
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = handle.state::<AppState>();
        let report = Reporter::new(handle.clone(), id.clone(), JobKind::Generate);
        let mut request = request;
        let key = state.settings.lock().unwrap().online_key();
        request.online = key.map(|key| asr::sarvam::Online { key, http: state.http.clone() });
        let result = jobs::guard(pipeline::generate(&state.paths, request, &id, &cancel, &report)).await;
        jobs::finish(&handle, &state.jobs, JobKind::Generate, &id, result);
    });
    Ok(job_id)
}

#[tauri::command]
async fn start_translate(app: AppHandle, state: State<'_, AppState>, request: pipeline::TranslateRequest) -> Cmd<String> {
    let (job_id, cancel) = state.jobs.begin(JobKind::Generate);
    let id = job_id.clone();
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = handle.state::<AppState>();
        let report = Reporter::new(handle.clone(), id.clone(), JobKind::Generate);
        let result = jobs::guard(pipeline::translate_existing(&state.paths, request, &id, &cancel, &report)).await;
        jobs::finish(&handle, &state.jobs, JobKind::Generate, &id, result);
    });
    Ok(job_id)
}

#[tauri::command]
fn cancel_job(state: State<AppState>, job_id: String) {
    state.jobs.cancel(&job_id);
}

#[tauri::command]
fn plan_translation(state: State<AppState>, source: String, target: String) -> translate::routes::PlanInfo {
    let installed = models::installed_argos_ids(&state.paths);
    translate::routes::describe(&source, &target, &installed)
}

// ---------- models ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WhisperEntry {
    #[serde(flatten)]
    model: catalog::WhisperModel,
    installed: bool,
    downloading: bool,
    partial_bytes: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ArgosEntry {
    #[serde(flatten)]
    package: catalog::ArgosPackage,
    installed: bool,
    downloading: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelState {
    whisper: Vec<WhisperEntry>,
    argos: Vec<ArgosEntry>,
    disk_usage_bytes: u64,
    free_bytes: Option<u64>,
    models_dir: String,
}

#[tauri::command]
fn model_state(state: State<AppState>) -> ModelState {
    let p = &state.paths;
    let active = state.jobs.active_downloads();
    let whisper = catalog::whisper()
        .models
        .iter()
        .map(|m| {
            let path = models::whisper_path(p, m);
            WhisperEntry {
                installed: models::whisper_installed(p, m),
                downloading: active.contains(&m.id),
                partial_bytes: std::fs::metadata(models::download::part_path(&path)).map(|md| md.len()).unwrap_or(0),
                model: m.clone(),
            }
        })
        .collect();
    let argos = catalog::argos()
        .packages
        .iter()
        .map(|pkg| ArgosEntry {
            installed: models::argos_installed(p, pkg),
            downloading: active.contains(&pkg.id),
            package: pkg.clone(),
        })
        .collect();
    ModelState {
        whisper,
        argos,
        disk_usage_bytes: models::disk_usage(p),
        free_bytes: paths::available_space(&p.models),
        models_dir: p.models.to_string_lossy().into_owned(),
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadEvent {
    id: String,
    /// downloading | verifying | installing | done | failed | cancelled
    state: &'static str,
    received: u64,
    total: u64,
    bytes_per_sec: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<AppError>,
}

#[tauri::command]
async fn download_model(app: AppHandle, state: State<'_, AppState>, id: String) -> Cmd<()> {
    let Some(cancel) = state.jobs.begin_download(&id) else { return Ok(()) };
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = handle.state::<AppState>();
        let emit = |s: &'static str, received: u64, total: u64, bps: f64, error: Option<AppError>| {
            let _ = handle.emit("download-progress", DownloadEvent { id: id.clone(), state: s, received, total, bytes_per_sec: bps, error });
        };
        let result = jobs::guard(install(&state, &id, &cancel, &emit)).await;
        state.jobs.end_download(&id);
        match result {
            Ok(total) => emit("done", total, total, 0.0, None),
            Err(e) if e.is_cancelled() => emit("cancelled", 0, 0, 0.0, None),
            Err(e) => emit("failed", 0, 0, 0.0, Some(e)),
        }
    });
    Ok(())
}

async fn install(
    state: &AppState,
    id: &str,
    cancel: &tokio_util::sync::CancellationToken,
    emit: &impl Fn(&'static str, u64, u64, f64, Option<AppError>),
) -> AppResult<u64> {
    let p = &state.paths;
    if let Some(m) = catalog::whisper_model(id) {
        let dest = models::whisper_path(p, m);
        models::download::download_verified(&state.http, &m.url, &dest, m.size_bytes, &m.sha256, cancel, |pr| {
            emit("downloading", pr.received, pr.total, pr.bytes_per_sec, None)
        })
        .await?;
        models::mark_whisper_verified(&dest, &m.sha256)?;
        return Ok(m.size_bytes);
    }
    let pkg = catalog::argos_package(id).ok_or_else(|| AppError::internal(format!("unknown model {id}")))?;
    let archive = p.downloads().join(format!("{}.argosmodel", pkg.id));
    models::download::download_verified(&state.http, &pkg.url, &archive, pkg.size_bytes, &pkg.sha256, cancel, |pr| {
        emit("downloading", pr.received, pr.total, pr.bytes_per_sec, None)
    })
    .await?;
    emit("installing", pkg.size_bytes, pkg.size_bytes, 0.0, None);
    paths::ensure_space(&p.argos_models(), pkg.size_bytes * 2, "to install this language")?;
    let dest = models::argos_dir(p, pkg);
    let archive2 = archive.clone();
    let dest2 = dest.clone();
    tokio::task::spawn_blocking(move || models::download::extract_argos_package(&archive2, &dest2))
        .await
        .map_err(|e| AppError::internal(e.to_string()))??;
    models::write_argos_marker(&dest, pkg)?;
    let _ = std::fs::remove_file(&archive);
    Ok(pkg.size_bytes)
}

#[tauri::command]
fn cancel_download(state: State<AppState>, id: String) {
    state.jobs.cancel_download(&id);
}

#[tauri::command]
fn remove_model(state: State<AppState>, id: String) -> Cmd<()> {
    if let Some(m) = catalog::whisper_model(&id) {
        return models::remove_whisper(&state.paths, m);
    }
    if let Some(pkg) = catalog::argos_package(&id) {
        let _ = std::fs::remove_file(models::download::part_path(&state.paths.downloads().join(format!("{}.argosmodel", pkg.id))));
        return models::remove_argos(&state.paths, pkg);
    }
    Err(AppError::internal(format!("unknown model {id}")))
}

// ---------- captions & export ----------

#[tauri::command]
fn validate_cues(cues: Vec<Cue>) -> validate::Report {
    validate::validate(&cues)
}

#[tauri::command]
fn fix_overlaps(mut cues: Vec<Cue>, min_ms: Option<i64>) -> Vec<Cue> {
    validate::fix_overlaps(&mut cues, min_ms.unwrap_or(300));
    cues
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportResult {
    path: String,
    cue_count: usize,
    bytes: usize,
}

#[tauri::command]
fn export_srt(path: String, cues: Vec<Cue>) -> Cmd<ExportResult> {
    let report = validate::validate(&cues);
    if report.has_blocking() {
        return Err(AppError::new(ErrorCode::ExportInvalid, "Some captions need fixing before export.")
            .with_detail(serde_json::to_string(&report.issues).unwrap_or_default()));
    }
    let mut sorted = cues;
    validate::sort_by_start(&mut sorted);
    let text = captions::srt::to_srt(&sorted);
    let target = PathBuf::from(&path);
    let dir = target.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    std::io::Write::write_all(&mut tmp, text.as_bytes())?;
    tmp.as_file().sync_all()?;
    project::persist(tmp, &target)?;
    Ok(ExportResult { path, cue_count: sorted.len(), bytes: text.len() })
}

#[tauri::command]
fn import_srt(path: String) -> Cmd<Vec<Cue>> {
    let bytes = std::fs::read(&path)?;
    let text = String::from_utf8(bytes).map_err(|_| AppError::new(ErrorCode::ProjectInvalid, "This SRT file isn't UTF-8 text."))?;
    captions::srt::parse_srt(&text).map_err(|e| AppError::new(ErrorCode::ProjectInvalid, "This SRT file couldn't be read.").with_detail(e))
}

// ---------- projects ----------

#[tauri::command]
fn save_project(app: AppHandle, state: State<AppState>, path: String, project: project::Project) -> Cmd<()> {
    project::save(Path::new(&path), &project)?;
    remember_project(&app, &state, &path, &project);
    Ok(())
}

#[tauri::command]
fn load_project(app: AppHandle, state: State<AppState>, path: String) -> Cmd<project::LoadedProject> {
    let loaded = project::load(Path::new(&path))?;
    if let project::VideoStatus::Found { path: v } | project::VideoStatus::Changed { path: v } = &loaded.video_status {
        allow_media(&app, Path::new(v));
    }
    remember_project(&app, &state, &path, &loaded.project);
    Ok(loaded)
}

fn remember_project(_app: &AppHandle, state: &AppState, path: &str, p: &project::Project) {
    let mut s = state.settings.lock().unwrap();
    s.touch_recent(settings::RecentProject {
        path: path.to_string(),
        name: Path::new(path).file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        video_name: p.video.file_name.clone(),
        opened_at_ms: project::now_ms(),
    });
    let _ = settings::save(&state.paths.config, &s);
}

#[tauri::command]
fn forget_recent(state: State<AppState>, path: String) -> Cmd<()> {
    let mut s = state.settings.lock().unwrap();
    s.recent_projects.retain(|r| r.path != path);
    settings::save(&state.paths.config, &s)
}

#[tauri::command]
fn path_exists(path: String) -> bool {
    Path::new(&path).exists()
}

#[tauri::command]
fn now_ms() -> i64 {
    project::now_ms()
}

// ---------- setup ----------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let self_test = selftest::SelfTest::from_args();
    let is_self_test = self_test.is_some();
    // Release builds have no console: put panics in the log file as well.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("panic: {info}");
        default_hook(info);
    }));

    let mut builder = tauri::Builder::default();
    // A test build driven through the automation bridge runs next to an installed copy.
    let test_instance = cfg!(feature = "automation") && std::env::var_os("CAPTIONNEST_AUTOMATION_PORT").is_some();
    if !is_self_test && !test_instance {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }));
    }
    builder
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .max_file_size(2_000_000)
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_window_state::Builder::new().build())
        .setup(move |app| {
            let paths = Paths::resolve(app.handle()).inspect_err(|e| {
                eprintln!("CaptionNest could not start: {} ({})", e.message, e.detail.as_deref().unwrap_or(""));
                log::error!("setup failed: {e:?}");
            })?;
            jobs::sweep_abandoned(&paths.jobs);
            #[cfg(feature = "automation")]
            automation::start(app.handle());
            let settings = settings::load(&paths.config);
            let voice = voice::VoiceKey::load(&paths.config);
            app.manage(AppState {
                paths,
                jobs: Jobs::default(),
                settings: Mutex::new(settings),
                http: models::download::client(),
                voice,
                self_test: is_self_test,
            });
            if let Some(test) = self_test.clone() {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    // A crash still ends the process with a failure code instead of hanging CI.
                    let code = jobs::guard(async { Ok::<_, AppError>(selftest::run(&handle, test).await) }).await.unwrap_or(3);
                    handle.exit(code);
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            show_main_window,
            get_settings,
            save_settings,
            probe_media,
            start_preview,
            start_generate,
            check_online_key,
            burn_begin,
            burn_frame,
            start_burn,
            start_translate,
            cancel_job,
            plan_translation,
            model_state,
            download_model,
            cancel_download,
            remove_model,
            validate_cues,
            fix_overlaps,
            export_srt,
            import_srt,
            save_project,
            load_project,
            forget_recent,
            path_exists,
            now_ms,
            list_fonts,
            import_fonts,
            font_file,
            remove_font,
            voice_status,
            save_voice_key,
            remove_voice_key,
            transcribe_voice,
        ])
        .run(tauri::generate_context!())
        .expect("error while running CaptionNest");
}
