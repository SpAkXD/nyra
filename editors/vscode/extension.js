// The Nyra extension: starts `nyra lsp` (the language server in the compiler) and connects it to
// VS Code. The client is written here, on Node's standard library only, so the extension ships
// without node_modules: JSON-RPC messages with a Content-Length header over the server's stdio,
// and each LSP result turned into the VS Code object it describes.

'use strict';

const vscode = require('vscode');
const cp = require('child_process');

/** A connection to one `nyra lsp` process. */
class Client {
    constructor(command, log, onNotification, onExit) {
        this.log = log;
        this.nextId = 1;
        this.pending = new Map();
        this.buffer = Buffer.alloc(0);
        this.closed = false;
        this.proc = cp.spawn(command, ['lsp'], { stdio: ['pipe', 'pipe', 'pipe'] });
        this.proc.stdout.on('data', (chunk) => this.receive(chunk));
        this.proc.stderr.on('data', (chunk) => log.append(chunk.toString()));
        this.proc.on('error', (err) => {
            this.closed = true;
            this.failAll(err);
            onExit(err);
        });
        this.proc.on('exit', (code) => {
            this.closed = true;
            this.failAll(new Error(`nyra lsp exited with code ${code}`));
            onExit(code === 0 ? null : new Error(`nyra lsp exited with code ${code}`));
        });
        this.proc.stdin.on('error', () => {});
        this.onNotification = onNotification;
    }

    failAll(err) {
        for (const { reject } of this.pending.values()) {
            reject(err);
        }
        this.pending.clear();
    }

    write(msg) {
        if (this.closed) {
            return;
        }
        const body = Buffer.from(JSON.stringify(msg), 'utf8');
        this.proc.stdin.write(`Content-Length: ${body.length}\r\n\r\n`);
        this.proc.stdin.write(body);
    }

    request(method, params, token) {
        if (this.closed) {
            return Promise.resolve(null);
        }
        const id = this.nextId++;
        return new Promise((resolve, reject) => {
            this.pending.set(id, { resolve, reject });
            this.write({ jsonrpc: '2.0', id, method, params });
            if (token) {
                token.onCancellationRequested(() => {
                    if (this.pending.delete(id)) {
                        this.notify('$/cancelRequest', { id });
                        resolve(null);
                    }
                });
            }
        });
    }

    notify(method, params) {
        this.write({ jsonrpc: '2.0', method, params });
    }

    receive(chunk) {
        this.buffer = Buffer.concat([this.buffer, chunk]);
        for (;;) {
            const end = this.buffer.indexOf('\r\n\r\n');
            if (end < 0) {
                return;
            }
            const headers = this.buffer.slice(0, end).toString('ascii');
            const m = /Content-Length:\s*(\d+)/i.exec(headers);
            if (!m) {
                this.log.appendLine(`nyra: a message without Content-Length: ${headers}`);
                this.buffer = this.buffer.slice(end + 4);
                continue;
            }
            const length = Number(m[1]);
            if (this.buffer.length < end + 4 + length) {
                return;
            }
            const body = this.buffer.slice(end + 4, end + 4 + length).toString('utf8');
            this.buffer = this.buffer.slice(end + 4 + length);
            let msg;
            try {
                msg = JSON.parse(body);
            } catch (e) {
                this.log.appendLine(`nyra: bad JSON from the server: ${e}`);
                continue;
            }
            this.dispatch(msg);
        }
    }

    dispatch(msg) {
        if (msg.id !== undefined && msg.method === undefined) {
            const p = this.pending.get(msg.id);
            if (p) {
                this.pending.delete(msg.id);
                if (msg.error) {
                    this.log.appendLine(`nyra: ${msg.error.message}`);
                    p.resolve(null);
                } else {
                    p.resolve(msg.result);
                }
            }
        } else if (msg.method !== undefined && msg.id === undefined) {
            this.onNotification(msg.method, msg.params);
        } else if (msg.method !== undefined) {
            // a request from the server: none is expected
            this.write({ jsonrpc: '2.0', id: msg.id, error: { code: -32601, message: 'not supported' } });
        }
    }

    async stop() {
        if (this.closed) {
            return;
        }
        const done = this.request('shutdown', null).catch(() => null);
        await Promise.race([done, new Promise((r) => setTimeout(r, 1000))]);
        this.notify('exit', null);
        this.closed = true;
        setTimeout(() => this.proc.kill(), 1000);
    }
}

// ---- LSP to VS Code --------------------------------------------------------------------------

const pos = (p) => new vscode.Position(p.line, p.character);
const range = (r) => new vscode.Range(pos(r.start), pos(r.end));
const lspPos = (p) => ({ line: p.line, character: p.character });
const lspRange = (r) => ({ start: lspPos(r.start), end: lspPos(r.end) });
const markdown = (value) => {
    const md = new vscode.MarkdownString(value);
    md.isTrusted = false;
    return md;
};
const docOf = (d) => (d && typeof d === 'object' ? markdown(d.value) : d);

const severities = [
    vscode.DiagnosticSeverity.Error,
    vscode.DiagnosticSeverity.Warning,
    vscode.DiagnosticSeverity.Information,
    vscode.DiagnosticSeverity.Hint,
];

function toDiagnostic(d) {
    const out = new vscode.Diagnostic(range(d.range), d.message, severities[(d.severity || 1) - 1]);
    out.source = d.source || 'nyra';
    if (d.code !== undefined) {
        out.code = d.codeDescription ? { value: d.code, target: vscode.Uri.parse(d.codeDescription.href) } : d.code;
    }
    if (d.relatedInformation) {
        out.relatedInformation = d.relatedInformation.map(
            (r) => new vscode.DiagnosticRelatedInformation(new vscode.Location(vscode.Uri.parse(r.location.uri), range(r.location.range)), r.message),
        );
    }
    return out;
}

function toEdit(changes) {
    const edit = new vscode.WorkspaceEdit();
    for (const uri of Object.keys(changes || {})) {
        for (const e of changes[uri]) {
            edit.replace(vscode.Uri.parse(uri), range(e.range), e.newText);
        }
    }
    return edit;
}

function toSymbol(s) {
    const sym = new vscode.DocumentSymbol(s.name, s.detail || '', s.kind - 1, range(s.range), range(s.selectionRange));
    sym.children = (s.children || []).map(toSymbol);
    return sym;
}

// ---- the extension ------------------------------------------------------------------------------

let client = null;
let starting = false;

function activate(context) {
    const log = vscode.window.createOutputChannel('Nyra');
    const diagnostics = vscode.languages.createDiagnosticCollection('nyra');
    context.subscriptions.push(log, diagnostics);
    const selector = { language: 'nyra' };
    const isNyra = (doc) => doc.languageId === 'nyra';
    const textDoc = (doc) => ({ uri: doc.uri.toString() });
    const at = (doc, p) => ({ textDocument: textDoc(doc), position: lspPos(p) });
    let warned = false;

    const open = (doc) => {
        if (client && isNyra(doc)) {
            client.notify('textDocument/didOpen', { textDocument: { uri: doc.uri.toString(), languageId: 'nyra', version: doc.version, text: doc.getText() } });
        }
    };

    const start = () => {
        if (!vscode.workspace.getConfiguration('nyra').get('lsp.enabled', true)) {
            return;
        }
        const command = vscode.workspace.getConfiguration('nyra').get('path', 'nyra') || 'nyra';
        starting = true;
        const me = new Client(
            command,
            log,
            (method, params) => {
                if (method === 'textDocument/publishDiagnostics') {
                    diagnostics.set(vscode.Uri.parse(params.uri), params.diagnostics.map(toDiagnostic));
                }
            },
            (err) => {
                if (client !== me) {
                    return;
                }
                client = null;
                diagnostics.clear();
                if (err && !warned) {
                    warned = true;
                    const why = err.code === 'ENOENT' ? `\`${command}\` was not found` : String(err.message || err);
                    vscode.window
                        .showWarningMessage(`Nyra: the language server did not start (${why}). Set "nyra.path" to the nyra executable (version 0.9 or later).`, 'Open Settings')
                        .then((choice) => {
                            if (choice) {
                                vscode.commands.executeCommand('workbench.action.openSettings', 'nyra.path');
                            }
                        });
                }
            },
        );
        client = me;
        me.request('initialize', {
            processId: process.pid,
            clientInfo: { name: 'vscode-nyra' },
            rootUri: vscode.workspace.workspaceFolders ? vscode.workspace.workspaceFolders[0].uri.toString() : null,
            capabilities: { general: { positionEncodings: ['utf-16'] } },
        }).then((result) => {
            starting = false;
            if (!result || client !== me) {
                return;
            }
            me.notify('initialized', {});
            vscode.workspace.textDocuments.forEach(open);
        });
    };

    const stop = async () => {
        const c = client;
        client = null;
        diagnostics.clear();
        if (c) {
            await c.stop();
        }
    };

    const ask = (method, params, token) => (client && !starting ? client.request(method, params, token).catch(() => null) : Promise.resolve(null));

    context.subscriptions.push(
        vscode.workspace.onDidOpenTextDocument(open),
        vscode.workspace.onDidChangeTextDocument((e) => {
            if (client && isNyra(e.document) && e.contentChanges.length > 0) {
                client.notify('textDocument/didChange', {
                    textDocument: { uri: e.document.uri.toString(), version: e.document.version },
                    contentChanges: [{ text: e.document.getText() }],
                });
            }
        }),
        vscode.workspace.onDidSaveTextDocument((doc) => {
            if (client && isNyra(doc)) {
                client.notify('textDocument/didSave', { textDocument: textDoc(doc) });
            }
        }),
        vscode.workspace.onDidCloseTextDocument((doc) => {
            if (client && isNyra(doc)) {
                client.notify('textDocument/didClose', { textDocument: textDoc(doc) });
                diagnostics.delete(doc.uri);
            }
        }),
        vscode.workspace.onWillSaveTextDocument((e) => {
            if (client && isNyra(e.document) && vscode.workspace.getConfiguration('nyra', e.document).get('formatOnSave', false)) {
                const edits = ask('textDocument/formatting', { textDocument: textDoc(e.document), options: { tabSize: 4, insertSpaces: true } }).then(
                    (r) => (r || []).map((t) => vscode.TextEdit.replace(range(t.range), t.newText)),
                );
                e.waitUntil(edits);
            }
        }),
        vscode.workspace.onDidChangeConfiguration(async (e) => {
            if (e.affectsConfiguration('nyra.path') || e.affectsConfiguration('nyra.lsp.enabled')) {
                warned = false;
                await stop();
                start();
            }
        }),
        vscode.commands.registerCommand('nyra.restartServer', async () => {
            warned = false;
            await stop();
            start();
        }),
        vscode.languages.registerHoverProvider(selector, {
            provideHover: async (doc, p, token) => {
                const r = await ask('textDocument/hover', at(doc, p), token);
                return r ? new vscode.Hover(markdown(r.contents.value)) : null;
            },
        }),
        vscode.languages.registerCompletionItemProvider(
            selector,
            {
                provideCompletionItems: async (doc, p, token) => {
                    const r = await ask('textDocument/completion', at(doc, p), token);
                    if (!r) {
                        return null;
                    }
                    return r.items.map((i) => {
                        const item = new vscode.CompletionItem(i.label, (i.kind || 1) - 1);
                        item.detail = i.detail;
                        item.documentation = docOf(i.documentation);
                        if (i.insertText) {
                            item.insertText = i.insertText;
                        }
                        if (i.sortText) {
                            item.sortText = i.sortText;
                        }
                        return item;
                    });
                },
            },
            '.',
        ),
        vscode.languages.registerSignatureHelpProvider(
            selector,
            {
                provideSignatureHelp: async (doc, p, token) => {
                    const r = await ask('textDocument/signatureHelp', at(doc, p), token);
                    if (!r || !r.signatures || r.signatures.length === 0) {
                        return null;
                    }
                    const help = new vscode.SignatureHelp();
                    help.signatures = r.signatures.map((s) => {
                        const info = new vscode.SignatureInformation(s.label, docOf(s.documentation));
                        info.parameters = (s.parameters || []).map((x) => new vscode.ParameterInformation(x.label));
                        return info;
                    });
                    help.activeSignature = r.activeSignature || 0;
                    help.activeParameter = r.activeParameter || 0;
                    return help;
                },
            },
            { triggerCharacters: ['(', ','], retriggerCharacters: [','] },
        ),
        vscode.languages.registerDefinitionProvider(selector, {
            provideDefinition: async (doc, p, token) => {
                const r = await ask('textDocument/definition', at(doc, p), token);
                return r ? new vscode.Location(vscode.Uri.parse(r.uri), range(r.range)) : null;
            },
        }),
        vscode.languages.registerDocumentSymbolProvider(selector, {
            provideDocumentSymbols: async (doc, token) => {
                const r = await ask('textDocument/documentSymbol', { textDocument: textDoc(doc) }, token);
                return r ? r.map(toSymbol) : null;
            },
        }),
        vscode.languages.registerDocumentFormattingEditProvider(selector, {
            provideDocumentFormattingEdits: async (doc, options, token) => {
                const r = await ask('textDocument/formatting', { textDocument: textDoc(doc), options: { tabSize: options.tabSize, insertSpaces: options.insertSpaces } }, token);
                return r ? r.map((t) => vscode.TextEdit.replace(range(t.range), t.newText)) : null;
            },
        }),
        vscode.languages.registerCodeActionsProvider(
            selector,
            {
                provideCodeActions: async (doc, rng, ctx, token) => {
                    const only = ctx.only ? [ctx.only.value] : undefined;
                    const r = await ask('textDocument/codeAction', { textDocument: textDoc(doc), range: lspRange(rng), context: { diagnostics: [], only } }, token);
                    if (!r) {
                        return null;
                    }
                    return r.map((a) => {
                        const kind = a.kind === 'source.fixAll' ? vscode.CodeActionKind.SourceFixAll : vscode.CodeActionKind.QuickFix;
                        const action = new vscode.CodeAction(a.title, kind);
                        action.edit = toEdit(a.edit && a.edit.changes);
                        action.isPreferred = !!a.isPreferred;
                        if (a.diagnostics) {
                            // the diagnostics VS Code shows that this action fixes
                            action.diagnostics = ctx.diagnostics.filter((d) =>
                                a.diagnostics.some((x) => range(x.range).isEqual(d.range) && String(x.code) === String(typeof d.code === 'object' ? d.code.value : d.code)),
                            );
                        }
                        return action;
                    });
                },
            },
            { providedCodeActionKinds: [vscode.CodeActionKind.QuickFix, vscode.CodeActionKind.SourceFixAll] },
        ),
        { dispose: () => stop() },
    );
    start();
}

function deactivate() {
    return client ? client.stop() : undefined;
}

module.exports = { activate, deactivate };
