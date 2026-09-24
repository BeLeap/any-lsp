const cp = require("child_process");
const fs = require("fs");
const path = require("path");
const vscode = require("vscode");

const FILE_SELECTOR = [{ scheme: "file", pattern: "**/*" }];
let activeManager;

class LspClient {
  constructor(context, rootPath, output) {
    this.context = context;
    this.rootPath = rootPath;
    this.output = output;
    this.buffer = Buffer.alloc(0);
    this.nextRequestId = 1;
    this.pending = new Map();
    this.openDocuments = new Set();
    this.startPromise = undefined;
    this.child = undefined;
  }

  start() {
    if (this.startPromise) {
      return this.startPromise;
    }

    this.startPromise = new Promise((resolve, reject) => {
      const command = this.serverCommand();
      this.output.appendLine(`Starting ${command} for ${this.rootPath}`);
      this.child = cp.spawn(command, ["--root", this.rootPath], {
        cwd: this.rootPath,
        stdio: ["pipe", "pipe", "pipe"]
      });

      let started = false;
      const failBeforeStart = (error) => {
        if (!started) {
          reject(error);
        }
      };

      this.child.once("spawn", () => {
        started = true;
        this.sendRequest("initialize", this.initializeParams())
          .then(() => {
            this.sendNotification("initialized", {});
            this.output.appendLine(
              `Successfully started ${command} for ${this.rootPath}`
            );
            resolve();
          })
          .catch(reject);
      });
      this.child.once("error", failBeforeStart);
      this.child.once("exit", (code, signal) => {
        const reason = new Error(
          `any-lsp exited for ${this.rootPath} (code=${code}, signal=${signal})`
        );
        for (const { reject: rejectPending } of this.pending.values()) {
          rejectPending(reason);
        }
        this.pending.clear();
        failBeforeStart(reason);
      });
      this.child.stdout.on("data", (chunk) => this.read(chunk));
      this.child.stderr.on("data", (chunk) => {
        this.output.append(chunk.toString());
      });
    });

    return this.startPromise;
  }

  serverCommand() {
    const configured = vscode.workspace
      .getConfiguration("anyLsp")
      .get("serverPath", "");
    if (configured) {
      return configured;
    }

    const architecture = {
      x64: "x86_64",
      arm64: "aarch64"
    }[process.arch] || process.arch;
    const bundledName = `any-lsp-${architecture}-${process.platform}`;
    const bundled = this.context.asAbsolutePath(path.join("bin", bundledName));
    return fs.existsSync(bundled) ? bundled : "any-lsp";
  }

  initializeParams() {
    const configuration = vscode.workspace.getConfiguration("anyLsp");
    const rootUri = vscode.Uri.file(this.rootPath).toString();
    return {
      processId: process.pid,
      rootPath: this.rootPath,
      rootUri,
      capabilities: {
        workspace: { workspaceFolders: true },
        textDocument: {
          definition: {},
          references: {}
        }
      },
      workspaceFolders: [
        {
          uri: rootUri,
          name: path.basename(this.rootPath)
        }
      ],
      initializationOptions: {
        maxResults: configuration.get("maxResults", 1000),
        caseSensitive: configuration.get("caseSensitive", true),
        include: configuration.get("include", []),
        exclude: configuration.get("exclude", [])
      }
    };
  }

  open(document) {
    return this.start().then(() => {
      if (this.openDocuments.has(document.uri.toString())) {
        return;
      }
      this.openDocuments.add(document.uri.toString());
      this.sendNotification("textDocument/didOpen", {
        textDocument: {
          uri: document.uri.toString(),
          languageId: document.languageId,
          version: document.version,
          text: document.getText()
        }
      });
    });
  }

  change(document) {
    return this.open(document).then(() => {
      this.sendNotification("textDocument/didChange", {
        textDocument: {
          uri: document.uri.toString(),
          version: document.version
        },
        contentChanges: [{ text: document.getText() }]
      });
    });
  }

  close(document) {
    return this.start().then(() => {
      const uri = document.uri.toString();
      if (!this.openDocuments.delete(uri)) {
        return;
      }
      this.sendNotification("textDocument/didClose", {
        textDocument: { uri }
      });
    });
  }

  request(method, params) {
    return this.start().then(() => this.sendRequest(method, params));
  }

  sendRequest(method, params) {
    const id = this.nextRequestId++;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      this.write({ jsonrpc: "2.0", id, method, params });
    });
  }

  sendNotification(method, params) {
    this.write({ jsonrpc: "2.0", method, params });
  }

  write(message) {
    const body = Buffer.from(JSON.stringify(message), "utf8");
    const header = Buffer.from(`Content-Length: ${body.length}\r\n\r\n`, "ascii");
    this.child.stdin.write(Buffer.concat([header, body]));
  }

  read(chunk) {
    this.buffer = Buffer.concat([this.buffer, chunk]);
    const separator = Buffer.from("\r\n\r\n", "ascii");
    while (true) {
      const headerEnd = this.buffer.indexOf(separator);
      if (headerEnd < 0) {
        return;
      }
      const headers = this.buffer.subarray(0, headerEnd).toString("ascii");
      const lengthMatch = headers.match(/(?:^|\r\n)Content-Length:\s*(\d+)/i);
      if (!lengthMatch) {
        throw new Error("any-lsp sent a message without Content-Length");
      }
      const bodyStart = headerEnd + separator.length;
      const bodyLength = Number(lengthMatch[1]);
      if (this.buffer.length < bodyStart + bodyLength) {
        return;
      }
      const body = this.buffer
        .subarray(bodyStart, bodyStart + bodyLength)
        .toString("utf8");
      this.buffer = this.buffer.subarray(bodyStart + bodyLength);
      this.handleMessage(JSON.parse(body));
    }
  }

  handleMessage(message) {
    if (message.id !== undefined && this.pending.has(message.id)) {
      const pending = this.pending.get(message.id);
      this.pending.delete(message.id);
      if (message.error) {
        pending.reject(new Error(message.error.message || "any-lsp request failed"));
      } else {
        pending.resolve(message.result);
      }
      return;
    }

    if (message.id !== undefined && message.method) {
      this.write({ jsonrpc: "2.0", id: message.id, result: null });
    }
  }

  async stop() {
    if (!this.child || this.child.exitCode !== null) {
      return;
    }
    try {
      await this.sendRequest("shutdown", null);
      this.sendNotification("exit", null);
    } catch (_) {
      // The process may already be gone while VS Code is shutting down.
    }
    this.child.kill();
  }
}

function toVscodeLocation(location) {
  const start = location && location.range && location.range.start;
  const end = location && location.range && location.range.end;
  const coordinates = start && end
    ? [start.line, start.character, end.line, end.character]
    : [];
  if (
    !location ||
    typeof location.uri !== "string" ||
    !coordinates.every(Number.isInteger)
  ) {
    throw new Error(`any-lsp returned an invalid location: ${JSON.stringify(location)}`);
  }

  return new vscode.Location(
    vscode.Uri.parse(location.uri),
    new vscode.Range(...coordinates)
  );
}

class ClientManager {
  constructor(context, output) {
    this.context = context;
    this.output = output;
    this.clients = new Map();
  }

  clientFor(document) {
    const folder = vscode.workspace.getWorkspaceFolder(document.uri);
    const rootPath = folder ? folder.uri.fsPath : path.dirname(document.uri.fsPath);
    const key = folder ? folder.uri.toString() : rootPath;
    let client = this.clients.get(key);
    if (!client) {
      client = new LspClient(this.context, rootPath, this.output);
      this.clients.set(key, client);
    }
    return client;
  }

  async open(document) {
    if (document.uri.scheme !== "file") {
      return;
    }
    await this.clientFor(document).open(document);
  }

  async change(document) {
    if (document.uri.scheme !== "file") {
      return;
    }
    await this.clientFor(document).change(document);
  }

  async close(document) {
    if (document.uri.scheme !== "file") {
      return;
    }
    await this.clientFor(document).close(document);
  }

  request(document, method, params) {
    const client = this.clientFor(document);
    const uri = params.textDocument.uri;
    const position = params.position;
    const source = position
      ? `${uri}:${position.line + 1}:${position.character + 1}`
      : uri;
    this.output.appendLine(`Request ${method} ${source}`);

    return client
      .open(document)
      .then(() => client.request(method, params))
      .then((result) => {
        if (!Array.isArray(result)) {
          throw new Error(
            `Expected a location array from ${method}, got ${JSON.stringify(result)}`
          );
        }
        const targets = result
          .slice(0, 3)
          .map((location) => {
            const start = location.range && location.range.start;
            return start
              ? `${location.uri}:${start.line + 1}:${start.character + 1}`
              : location.uri;
          })
          .join(", ");
        this.output.appendLine(
          `Response ${method}: ${result.length} location(s)${targets ? `; ${targets}` : ""}`
        );
        return result.map(toVscodeLocation);
      })
      .catch((error) => {
        this.output.appendLine(
          `Error ${method} for ${source}: ${error.stack || error}`
        );
        throw error;
      });
  }

  async stop() {
    await Promise.all([...this.clients.values()].map((client) => client.stop()));
  }
}

function activate(context) {
  const output = vscode.window.createOutputChannel("any-lsp");
  const manager = new ClientManager(context, output);
  activeManager = manager;
  const selector = FILE_SELECTOR;

  context.subscriptions.push(
    output,
    vscode.languages.registerDefinitionProvider(selector, {
      provideDefinition(document, position) {
        return manager.request(document, "textDocument/definition", {
          textDocument: { uri: document.uri.toString() },
          position: { line: position.line, character: position.character }
        });
      }
    }),
    vscode.languages.registerReferenceProvider(selector, {
      provideReferences(document, position, referenceContext) {
        return manager.request(document, "textDocument/references", {
          textDocument: { uri: document.uri.toString() },
          position: { line: position.line, character: position.character },
          context: {
            includeDeclaration: referenceContext.includeDeclaration
          }
        });
      }
    }),
    vscode.workspace.onDidOpenTextDocument((document) => {
      manager.open(document).catch((error) => output.appendLine(String(error)));
    }),
    vscode.workspace.onDidChangeTextDocument((event) => {
      manager.change(event.document).catch((error) => output.appendLine(String(error)));
    }),
    vscode.workspace.onDidCloseTextDocument((document) => {
      manager.close(document).catch((error) => output.appendLine(String(error)));
    })
  );

  for (const document of vscode.workspace.textDocuments) {
    manager.open(document).catch((error) => output.appendLine(String(error)));
  }
}

async function deactivate() {
  if (activeManager) {
    const manager = activeManager;
    activeManager = undefined;
    await manager.stop();
  }
}

module.exports = { activate, deactivate };
