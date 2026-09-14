import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

import { gitEnvironment, isolatedGit } from "./adr-files.mjs";
import {
  INDUSTRIAL_BINARIES,
  executableName,
  readIndustrialBundle,
} from "./install-servers.mjs";

const TARGETS = {
  "aarch64-apple-darwin": { platform: "darwin", arch: "arm64" },
  "x86_64-apple-darwin": { platform: "darwin", arch: "x64" },
  "x86_64-unknown-linux-gnu": { platform: "linux", arch: "x64" },
  "x86_64-pc-windows-msvc": { platform: "win32", arch: "x64" },
};

export function packageIndustrialBundle(
  { workspace = process.cwd(), target, out } = {},
  execute = execFileSync,
) {
  target ??= Object.keys(TARGETS).find((candidate) => {
    const value = TARGETS[candidate];
    return value.platform === process.platform && value.arch === process.arch;
  });
  if (!Object.hasOwn(TARGETS, target ?? "")) {
    throw new Error("unsupported Industrial host target");
  }
  const { platform, arch } = TARGETS[target];
  workspace = path.resolve(workspace);
  const capture = (command, args) => {
    const output =
      command === "git"
        ? isolatedGit(args, workspace, execute)
        : execute(command, args, {
            cwd: workspace,
            encoding: "utf8",
            env: gitEnvironment(),
          }).trim();
    if (output === null) {
      throw new Error("cannot read Industrial source checkout with Git");
    }
    return output;
  };
  const clean = (excludedDirectory) => {
    const relative =
      excludedDirectory && path.relative(workspace, excludedDirectory);
    const pathspec =
      relative &&
      relative !== ".." &&
      !relative.startsWith(`..${path.sep}`) &&
      !path.isAbsolute(relative)
        ? ["--", ".", `:(exclude,literal)${relative.split(path.sep).join("/")}`]
        : [];
    if (
      capture("git", [
        "status",
        "--porcelain",
        "--untracked-files=normal",
        ...pathspec,
      ])
    ) {
      throw new Error("Industrial bundles require a clean committed checkout");
    }
  };
  clean();
  const revision = capture("git", ["rev-parse", "HEAD"]);
  if (!/^[0-9a-f]{40}$/.test(revision))
    throw new Error("invalid source revision");
  const metadata = JSON.parse(
    capture("cargo", [
      "metadata",
      "--locked",
      "--no-deps",
      "--format-version",
      "1",
    ]),
  );
  const packages = INDUSTRIAL_BINARIES.map((name) =>
    name === "register-me" ? "ackplane-server" : name,
  );
  const versions = packages.map(
    (name) => metadata.packages.find((value) => value.name === name)?.version,
  );
  const version = versions[0];
  if (!version || versions.some((value) => value !== version)) {
    throw new Error("Industrial host packages must have one shared version");
  }
  const destination = path.resolve(
    workspace,
    out ?? `dist/mindleak-industrial-v${version}-${target}.zip`,
  );
  if (path.extname(destination) !== ".zip")
    throw new Error("--out must name a .zip archive");
  if (fs.existsSync(destination))
    throw new Error("Industrial bundle output already exists");
  execute(
    "cargo",
    [
      "build",
      "--locked",
      "--release",
      "--target",
      target,
      ...packages.flatMap((name) => ["-p", name]),
      "--features",
      "mindleak-mcp/federation-client,lodestar-mcp/federation-client",
    ],
    { cwd: workspace, stdio: "inherit", env: gitEnvironment() },
  );
  clean();
  if (capture("git", ["rev-parse", "HEAD"]) !== revision) {
    throw new Error(
      "source revision changed while building the Industrial bundle",
    );
  }

  fs.mkdirSync(path.dirname(destination), { recursive: true });
  const staging = fs.mkdtempSync(
    path.join(path.dirname(destination), ".industrial-bundle-"),
  );
  try {
    const files = INDUSTRIAL_BINARIES.map((binary) => {
      const name = executableName(binary, platform);
      const source = path.join(
        metadata.target_directory,
        target,
        "release",
        name,
      );
      if (!fs.lstatSync(source).isFile())
        throw new Error(`bundle input is not a regular file: ${name}`);
      const staged = path.join(staging, name);
      fs.copyFileSync(source, staged);
      if (platform !== "win32") fs.chmodSync(staged, 0o755);
      const bytes = fs.readFileSync(staged);
      return {
        name,
        size: bytes.length,
        sha256: createHash("sha256").update(bytes).digest("hex"),
      };
    });
    const smoke = fs.mkdtempSync(path.join(staging, ".smoke-"));
    try {
      const options = {
        cwd: smoke,
        encoding: "utf8",
        timeout: 15_000,
        env: {
          PATH: process.env.PATH,
          SystemRoot: process.env.SystemRoot,
          HOME: smoke,
          USERPROFILE: smoke,
          APPDATA: smoke,
          LOCALAPPDATA: smoke,
          TMPDIR: smoke,
          TMP: smoke,
          TEMP: smoke,
          MINDLEAK_WORKSPACE: smoke,
          MINDLEAK_COORDINATION_MODE: "local",
          MINDLEAK_DB: path.join(smoke, "graph.db"),
          LODESTAR_DB: path.join(smoke, "spec.db"),
        },
      };
      for (const binary of INDUSTRIAL_BINARIES) {
        const executable = path.join(staging, executableName(binary, platform));
        if (binary.endsWith("-mcp")) {
          const input =
            JSON.stringify({
              jsonrpc: "2.0",
              id: 1,
              method: "initialize",
              params: {
                protocolVersion: "2024-11-05",
                capabilities: {},
                clientInfo: { name: "industrial-bundle", version: "1" },
              },
            }) + "\n";
          const output = execute(executable, [], { ...options, input });
          const identity = output
            .trim()
            .split(/\r?\n/)
            .map((line) => JSON.parse(line))
            .find((reply) => reply.id === 1)?.result?.serverInfo;
          if (
            identity?.name !== binary ||
            identity.version?.split("+")[0] !== version
          ) {
            throw new Error(`unexpected installed MCP identity for ${binary}`);
          }
          if (
            (binary === "mindleak-mcp" || binary === "lodestar-mcp") &&
            identity.version !== `${version}+${revision.slice(0, 12)}`
          ) {
            throw new Error(
              `unexpected installed MCP source revision for ${binary}`,
            );
          }
        } else {
          let output;
          try {
            output = execute(executable, ["--help"], options);
          } catch (error) {
            if (binary !== "register-me" || error.status !== 1) throw error;
            output = String(error.stdout) + String(error.stderr);
          }
          if (!/usage:/i.test(output))
            throw new Error(`${binary} did not report CLI usage`);
        }
      }
    } finally {
      fs.rmSync(smoke, { recursive: true, force: true });
    }
    const manifest = {
      schema: 1,
      profile: "industrial",
      version,
      revision,
      platform,
      arch,
      files,
    };
    fs.writeFileSync(
      path.join(staging, "industrial-manifest.json"),
      JSON.stringify(manifest, null, 2) + "\n",
    );
    fs.copyFileSync(
      path.join(workspace, "scripts", "install-servers.mjs"),
      path.join(staging, "install.mjs"),
    );
    fs.copyFileSync(
      path.join(workspace, "LICENSE"),
      path.join(staging, "LICENSE"),
    );
    fs.writeFileSync(
      path.join(staging, "README.txt"),
      [
        `MindLeak Industrial host binaries ${version} (${target})`,
        `Source revision: ${revision}`,
        "",
        "Requirements: Node.js 20+ and a supported native OS credential facility.",
        "From the extracted directory: node install.mjs --profile industrial --bundle .",
        "The installer verifies every binary before replacing commands in ~/.mindleak/bin.",
        "No Git checkout or Rust toolchain is needed to install this bundle.",
        "Stop the companion and consumers before an upgrade, then restart them.",
        "No service is started and no enrollment or credential is created or replaced.",
        "Use the same installed register-me for enrollment and serving.",
        "The six replacements are not a single filesystem transaction; retry a failed install before restarting consumers.",
        "The manifest checks integrity, not publisher authenticity. Verify the release checksum and provenance before running install.mjs.",
        "These binaries are not OS publisher-signed. Linux requires glibc and an available Secret Service credential store.",
        "Published Linux bundles are built on Ubuntu 22.04; use a compatible glibc environment.",
        "The shared server and Bridge remain separately deployed through Compose; this is not production authentication.",
        `Setup: https://github.com/monk-eee/MindLeak/blob/${revision}/docs/INDUSTRIAL-QUICKSTART.md`,
        "",
      ].join("\n"),
    );
    readIndustrialBundle(staging, platform, arch);
    const archive = path.join(staging, "bundle.zip");
    execute(
      process.execPath,
      [
        path.join(
          workspace,
          "editors",
          "vscode",
          "scripts",
          "package-bundle.mjs",
        ),
        "--source",
        staging,
        "--out",
        archive,
        "--executables",
        ...files.map((file) => file.name),
        "--files",
        ...files.map((file) => file.name),
        "install.mjs",
        "industrial-manifest.json",
        "LICENSE",
        "README.txt",
      ],
      { cwd: workspace, stdio: "inherit", env: gitEnvironment() },
    );
    clean(staging);
    if (capture("git", ["rev-parse", "HEAD"]) !== revision) {
      throw new Error(
        "source revision changed while packaging the Industrial bundle",
      );
    }
    fs.linkSync(archive, destination);
    return { destination, manifest };
  } finally {
    fs.rmSync(staging, { recursive: true, force: true });
  }
}

if (
  process.argv[1] &&
  fs.realpathSync(process.argv[1]) ===
    fs.realpathSync(fileURLToPath(import.meta.url))
) {
  try {
    const { values } = parseArgs({
      options: {
        target: { type: "string" },
        out: { type: "string" },
        help: { type: "boolean", short: "h" },
      },
    });
    if (values.help) {
      console.log(
        "Usage: node scripts/industrial-bundle.mjs [--target RUST_TARGET] [--out ARCHIVE.zip]",
      );
    } else {
      const result = packageIndustrialBundle(values);
      console.log(
        `Industrial ${result.manifest.version} at ${result.manifest.revision}: ${result.destination}`,
      );
    }
  } catch (error) {
    console.error(`industrial-bundle: ${error.message}`);
    process.exitCode = 1;
  }
}
