use log::{error, info};
use serde::{de, Deserialize, Serialize};
use std::collections::HashMap;
use tauri::{AppHandle, Manager, State};
use thiserror::Error;
use url::Url;

use crate::{
    ui_messages::{ShowToastMsg, ToastStatus},
    AppState, UiMessage,
};

// Should match the one from "tauri.config.json" and "Info.plist"
const APP_IDENTIFIER: &str = "sh.loft.devpod";
const APP_URL_SCHEME: &str = "devpod";

pub struct CustomProtocol;

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone)]
pub struct OpenWorkspaceMsg {
    #[serde(rename(deserialize = "workspace"))]
    workspace_id: Option<String>,
    #[serde(rename(deserialize = "provider"))]
    provider_id: Option<String>,
    ide: Option<String>,
    source: Option<String>,
}

#[derive(Debug, PartialEq, Serialize, Clone)]
pub struct ImportWorkspaceMsg {
    workspace_id: String,
    workspace_uid: String,
    devpod_pro_host: String,
    options: HashMap<String, String>,
}

impl<'de> Deserialize<'de> for ImportWorkspaceMsg {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let mut options = HashMap::deserialize(deserializer)?;

        let workspace_id = options
            .remove("workspace-id")
            .ok_or_else(|| de::Error::missing_field("workspace-id"))?;

        let workspace_uid = options
            .remove("workspace-uid")
            .ok_or_else(|| de::Error::missing_field("workspace-uid"))?;

        let devpod_pro_host = options
            .remove("devpod-pro-host")
            .ok_or_else(|| de::Error::missing_field("devpod-pro-host"))?;

        Ok(ImportWorkspaceMsg {
            workspace_id,
            workspace_uid,
            devpod_pro_host,
            options,
        })
    }
}

#[derive(Error, Debug, Clone, Serialize)]
pub enum ParseError {
    #[error("Unsupported host: {0}")]
    UnsupportedHost(String),
    #[error("Unsupported query arguments: {0}")]
    InvalidQuery(String),
}

impl OpenWorkspaceMsg {
    pub fn empty() -> OpenWorkspaceMsg {
        OpenWorkspaceMsg {
            workspace_id: None,
            provider_id: None,
            ide: None,
            source: None,
        }
    }
    pub fn with_id(id: String) -> OpenWorkspaceMsg {
        OpenWorkspaceMsg {
            workspace_id: Some(id),
            provider_id: None,
            ide: None,
            source: None,
        }
    }
}

pub struct Request {
    host: String,
    query: String,
}

pub struct UrlParser {}

impl UrlParser {
    const ALLOWED_METHODS: [&'static str; 2] = ["open", "import"];

    fn get_host(url: &Url) -> String {
        url.host_str().unwrap_or("no host").to_string()
    }

    fn parse_raw_url(url_scheme: &str) -> Result<Url, ParseError> {
        Url::parse(url_scheme).map_err(|_| ParseError::InvalidQuery(url_scheme.to_string()))
    }

    fn is_allowed_method(host_str: &str) -> bool {
        Self::ALLOWED_METHODS.contains(&host_str)
    }

    fn parse_query(url: &Url) -> String {
        url.query().unwrap_or("").to_string()
    }

    pub fn parse(url_scheme: &str) -> Result<Request, ParseError> {
        let url = Self::parse_raw_url(url_scheme)?;
        let host_str = Self::get_host(&url);

        if !Self::is_allowed_method(&host_str) {
            return Err(ParseError::UnsupportedHost(host_str));
        }
        return Ok(Request {
            host: host_str,
            query: Self::parse_query(&url),
        });
    }
}

async fn send_ui_message(app_state: State<'_, AppState>, msg: UiMessage, log_msg_on_failure: &str) {
    if let Err(err) = app_state.ui_messages.send(msg).await {
        error!("{}: {:?}, {}", log_msg_on_failure, err.0, err);
    };
}

pub struct OpenHandler {}

impl OpenHandler {
    pub async fn handle(msg: Result<OpenWorkspaceMsg, ParseError>, app_state: State<'_, AppState>) {
        match msg {
            Ok(msg) => Self::handle_ok(msg, app_state).await,
            Err(err) => Self::handle_error(err, app_state).await,
        }
    }

    async fn handle_ok(msg: OpenWorkspaceMsg, app_state: State<'_, AppState>) {
        // try to send to UI if ready, otherwise buffer and let ui_ready handle
        send_ui_message(
            app_state,
            UiMessage::OpenWorkspace(msg),
            "Failed to broadcast custom protocol message",
        )
        .await;
    }

    async fn handle_error(err: ParseError, app_state: State<'_, AppState>) {
        #[cfg(not(target_os = "windows"))]
        send_ui_message(
            app_state,
            UiMessage::CommandFailed(err),
            "Failed to broadcast invalid custom protocol message",
        )
        .await;
    }
}

pub struct ImportHandler {}

impl ImportHandler {
    pub async fn handle(
        msg: Result<ImportWorkspaceMsg, ParseError>,
        app_state: State<'_, AppState>,
    ) {
        match msg {
            Ok(msg) => Self::handle_ok(msg, app_state).await,
            Err(err) => Self::handle_error(err, app_state).await,
        }
    }

    async fn handle_ok(msg: ImportWorkspaceMsg, app_state: State<'_, AppState>) {
        // try to send to UI if ready, otherwise buffer and let ui_ready handle
        send_ui_message(
            app_state,
            UiMessage::ImportWorkspace(msg),
            "Failed to broadcast custom protocol message",
        )
        .await;
    }

    async fn handle_error(err: ParseError, app_state: State<'_, AppState>) {
        #[cfg(not(target_os = "windows"))]
        send_ui_message(
            app_state,
            UiMessage::CommandFailed(err),
            "Failed to broadcast invalid custom protocol message",
        )
        .await;
    }
}

impl CustomProtocol {
    pub fn init() -> Self {
        tauri_plugin_deep_link::prepare(APP_IDENTIFIER);
        Self {}
    }

    pub fn setup(&self, app: AppHandle) {
        let app_handle = app.clone();

        let result = tauri_plugin_deep_link::register(APP_URL_SCHEME, move |url_scheme| {
            tauri::async_runtime::block_on(async {
                info!("App opened with URL: {:?}", url_scheme.to_string());

                let request = UrlParser::parse(&url_scheme.to_string());
                let app_state = app_handle.state::<AppState>();
                if let Err(err) = request {
                    #[cfg(not(target_os = "windows"))]
                    send_ui_message(
                        app_state,
                        UiMessage::CommandFailed(err),
                        "Failed to broadcast custom protocol message",
                    )
                    .await;
                    return;
                }
                let request = request.unwrap();

                match request.host.as_str() {
                    "open" => {
                        let msg = CustomProtocol::parse(&request);
                        OpenHandler::handle(msg, app_state).await
                    }

                    "import" => {
                        let msg = CustomProtocol::parse(&request);
                        ImportHandler::handle(msg, app_state).await
                    }
                    _ => {}
                }
            })
        });

        #[cfg(target_os = "linux")]
        {
            match result {
                Ok(..) => {}
                Err(error) => {
                    let msg = "Either update-desktop-database or xdg-mime are missing. Please make sure they are available on your system";
                    log::warn!("Custom protocol setup failed; {}: {}", msg, error);

                    tauri::async_runtime::block_on(async {
                        let app_state = app.state::<AppState>();
                        let show_toast_msg = ShowToastMsg::new(
                            "Custom protocol handling needs to be configured".to_string(),
                            msg.to_string(),
                            ToastStatus::Warning,
                        );
                        if let Err(err) = app_state
                            .ui_messages
                            .send(UiMessage::ShowToast(show_toast_msg))
                            .await
                        {
                            log::error!(
                                "Failed to broadcast show toast message: {:?}, {}",
                                err.0,
                                err
                            );
                        };
                    })
                }
            };
        }

        let _ = result;
    }

    fn parse<'a, Msg>(request: &'a Request) -> Result<Msg, ParseError>
    where
        Msg: Deserialize<'a>,
    {
        serde_qs::from_str::<Msg>(&request.query)
            .map_err(|_| ParseError::InvalidQuery(request.query.clone()))
    }
}

#[cfg(test)]
mod tests {
    mod url_parser {
        use super::super::*;

        #[test]
        fn should_parse() {
            let url_str = "devpod://open?workspace=workspace";
            let request = UrlParser::parse(&url_str).unwrap();

            assert_eq!(request.host, "open".to_string());
            assert_eq!(request.query, "workspace=workspace".to_string());
        }

        #[test]
        fn should_parse_with_empty_query() {
            let url_str = "devpod://import";
            let request = UrlParser::parse(&url_str).unwrap();

            assert_eq!(request.host, "import".to_string());
            assert_eq!(request.query, "".to_string());
        }

        #[test]
        #[should_panic]
        fn should_fail_on_invalid_method() {
            let url_str = "devpod://something";
            let _ = UrlParser::parse(&url_str).unwrap();
        }

        #[test]
        #[should_panic]
        fn should_fail_on_invalid_scheme() {
            let url_str = "invalid-scheme";
            let _ = UrlParser::parse(&url_str).unwrap();
        }
    }

    mod custom_handler_open {
        use crate::custom_protocol::OpenWorkspaceMsg;

        use super::super::*;

        #[test]
        fn should_parse_full() {
            let url_str =
                "devpod://open?workspace=workspace&provider=provider&source=https://github.com/test123&ide=vscode";
            let request = UrlParser::parse(&url_str).unwrap();
            let got: OpenWorkspaceMsg = CustomProtocol::parse(&request).unwrap();

            assert_eq!(got.workspace_id, Some("workspace".to_string()));
            assert_eq!(got.provider_id, Some("provider".into()));
            assert_eq!(got.source, Some("https://github.com/test123".to_string()));
            assert_eq!(got.ide, Some("vscode".into()));
        }

        #[test]
        fn should_parse_workspace() {
            let url_str = "devpod://open?workspace=some-workspace";
            let request = UrlParser::parse(&url_str).unwrap();
            let got: OpenWorkspaceMsg = CustomProtocol::parse(&request).unwrap();

            assert_eq!(got.workspace_id, Some("some-workspace".to_string()));
            assert_eq!(got.provider_id, None);
            assert_eq!(got.source, None);
            assert_eq!(got.ide, None)
        }

        #[test]
        fn should_parse() {
            let url_str = "devpod://open?source=some-source";
            let request = UrlParser::parse(&url_str).unwrap();
            let got: OpenWorkspaceMsg = CustomProtocol::parse(&request).unwrap();

            assert_eq!(got.workspace_id, None);
            assert_eq!(got.provider_id, None);
            assert_eq!(got.source, Some("some-source".to_string()));
            assert_eq!(got.ide, None)
        }
    }

    mod custom_handler_import {
        use crate::custom_protocol::ImportWorkspaceMsg;

        use super::super::*;

        #[test]
        fn should_parse_full() {
            let url_str =
                "devpod://import?workspace-id=workspace&workspace-uid=uid&devpod-pro-host=devpod.pro&other=other";
            let request = UrlParser::parse(&url_str).unwrap();

            let got: ImportWorkspaceMsg = CustomProtocol::parse(&request).unwrap();

            assert_eq!(got.workspace_id, "workspace".to_string());
            assert_eq!(got.workspace_uid, "uid".to_string());
            assert_eq!(got.devpod_pro_host, "devpod.pro".to_string());
            assert_eq!(got.options.get("other"), Some(&"other".to_string()));
        }

        #[test]
        #[should_panic]
        fn should_fail_on_missing_workspace_id() {
            let url_str =
                "devpod://import?workspace-uid=uid&devpod-pro-host=devpod.pro&other=other";
            let request = UrlParser::parse(&url_str).unwrap();

            let got: Result<ImportWorkspaceMsg, ParseError> = CustomProtocol::parse(&request);
            got.unwrap();
        }
    }
}
 
