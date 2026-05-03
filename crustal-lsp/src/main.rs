use std::collections::HashMap;

use crustal_lsp::{analyze, hover_at};
use lsp_server::{
    Connection, Message, Notification as LspNotification, Request as LspRequest, RequestId,
    Response,
};
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Notification,
};
use lsp_types::request::{HoverRequest, Request};
use lsp_types::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    HoverParams, InitializeParams, ServerCapabilities, TextDocumentSyncCapability,
    TextDocumentSyncKind, Url,
};
use serde_json::Value;

fn main() {
    let (connection, io_threads) = Connection::stdio();

    let capabilities = ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        hover_provider: Some(lsp_types::HoverProviderCapability::Simple(true)),
        ..ServerCapabilities::default()
    };
    let initialize_params = connection
        .initialize(serde_json::to_value(capabilities).unwrap())
        .unwrap();
    let _: InitializeParams = serde_json::from_value(initialize_params).unwrap_or_default();

    let mut server = Server::default();
    while let Ok(message) = connection.receiver.recv() {
        match message {
            Message::Request(request) => {
                if connection.handle_shutdown(&request).unwrap() {
                    break;
                }
                server.handle_request(&connection, request);
            }
            Message::Notification(notification) => {
                server.handle_notification(&connection, notification)
            }
            Message::Response(_) => {}
        }
    }

    drop(connection);
    io_threads.join().unwrap();
}

#[derive(Default)]
struct Server {
    documents: HashMap<Url, String>,
}

impl Server {
    fn handle_notification(&mut self, connection: &Connection, notification: LspNotification) {
        match notification.method.as_str() {
            DidOpenTextDocument::METHOD => {
                if let Ok(params) =
                    serde_json::from_value::<DidOpenTextDocumentParams>(notification.params)
                {
                    let uri = params.text_document.uri;
                    let text = params.text_document.text;
                    self.documents.insert(uri.clone(), text);
                    self.publish_diagnostics(connection, uri);
                }
            }
            DidChangeTextDocument::METHOD => {
                if let Ok(params) =
                    serde_json::from_value::<DidChangeTextDocumentParams>(notification.params)
                {
                    if let Some(change) = params.content_changes.into_iter().last() {
                        let uri = params.text_document.uri;
                        self.documents.insert(uri.clone(), change.text);
                        self.publish_diagnostics(connection, uri);
                    }
                }
            }
            DidCloseTextDocument::METHOD => {
                if let Ok(params) =
                    serde_json::from_value::<DidCloseTextDocumentParams>(notification.params)
                {
                    let uri = params.text_document.uri;
                    self.documents.remove(&uri);
                    let params = lsp_types::PublishDiagnosticsParams {
                        uri,
                        diagnostics: Vec::new(),
                        version: None,
                    };
                    send_notification(connection, "textDocument/publishDiagnostics", params);
                }
            }
            _ => {}
        }
    }

    fn handle_request(&mut self, connection: &Connection, request: LspRequest) {
        match request.method.as_str() {
            HoverRequest::METHOD => {
                let id = request.id;
                let result = serde_json::from_value::<HoverParams>(request.params)
                    .ok()
                    .and_then(|params| {
                        let uri = params.text_document_position_params.text_document.uri;
                        let position = params.text_document_position_params.position;
                        self.documents
                            .get(&uri)
                            .and_then(|text| hover_at(text, position))
                    });
                send_response(connection, id, result);
            }
            _ => send_response::<Value>(connection, request.id, None),
        }
    }

    fn publish_diagnostics(&self, connection: &Connection, uri: Url) {
        let diagnostics = self
            .documents
            .get(&uri)
            .map(|text| analyze(text).diagnostics)
            .unwrap_or_default();
        let params = lsp_types::PublishDiagnosticsParams {
            uri,
            diagnostics,
            version: None,
        };
        send_notification(connection, "textDocument/publishDiagnostics", params);
    }
}

fn send_notification<T: serde::Serialize>(connection: &Connection, method: &str, params: T) {
    let notification = LspNotification {
        method: method.to_string(),
        params: serde_json::to_value(params).unwrap(),
    };
    connection
        .sender
        .send(Message::Notification(notification))
        .unwrap();
}

fn send_response<T: serde::Serialize>(connection: &Connection, id: RequestId, result: Option<T>) {
    let response = Response {
        id,
        result: Some(serde_json::to_value(result).unwrap()),
        error: None,
    };
    connection.sender.send(Message::Response(response)).unwrap();
}
