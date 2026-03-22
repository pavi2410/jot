use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct InitOptions {
    pub template: Option<String>,
    pub group: Option<String>,
    pub package_name: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug)]
pub struct InitOutput {
    pub template: &'static str,
    pub root: PathBuf,
    pub created_files: usize,
}

#[derive(Debug, Clone, Copy)]
#[allow(clippy::enum_variant_names)]
enum TemplateKind {
    JavaMinimal,
    JavaLib,
    JavaCli,
    JavaServer,
    JavaWorkspace,
}

pub fn scaffold(cwd: &Path, options: InitOptions) -> Result<InitOutput, Box<dyn Error>> {
    let template = parse_template(options.template.as_deref())?;
    let group = options.group.unwrap_or_else(|| "com.example".to_owned());
    validate_package_name(&group)?;

    let package_name = match options.package_name {
        Some(value) => {
            validate_package_name(&value)?;
            value
        }
        None => group.clone(),
    };

    let project_name = options
        .name
        .unwrap_or_else(|| default_project_name(template).to_owned());
    validate_simple_name(&project_name)?;

    let root = cwd.join(&project_name);
    if root.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("destination already exists: {}", root.display()),
        )
        .into());
    }

    let files = render_template(template, &project_name, &group, &package_name)?;
    for (relative_path, content) in &files {
        let path = root.join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, content)?;
    }

    Ok(InitOutput {
        template: template.name(),
        root,
        created_files: files.len(),
    })
}

fn parse_template(value: Option<&str>) -> Result<TemplateKind, io::Error> {
    let normalized = value.unwrap_or("java-minimal").trim();
    let template = match normalized {
        "java-minimal" | "minimal" => TemplateKind::JavaMinimal,
        "java-lib" | "lib" => TemplateKind::JavaLib,
        "java-cli" | "cli" => TemplateKind::JavaCli,
        "java-server" | "server" => TemplateKind::JavaServer,
        "java-workspace" | "workspace" => TemplateKind::JavaWorkspace,
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "unknown template `{normalized}` (expected one of: java-minimal, java-lib, java-cli, java-server, java-workspace)"
                ),
            ));
        }
    };
    Ok(template)
}

fn default_project_name(template: TemplateKind) -> &'static str {
    match template {
        TemplateKind::JavaMinimal => "hello-jot",
        TemplateKind::JavaLib => "demo-library",
        TemplateKind::JavaCli => "demo-cli",
        TemplateKind::JavaServer => "demo-server",
        TemplateKind::JavaWorkspace => "shopflow",
    }
}

fn validate_simple_name(value: &str) -> Result<(), io::Error> {
    if value.trim().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "project name cannot be empty",
        ));
    }
    if value.contains('/') || value.contains('\\') || value == "." || value == ".." {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "project name must be a single path segment",
        ));
    }
    Ok(())
}

fn validate_package_name(value: &str) -> Result<(), io::Error> {
    if value.trim().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "package/group cannot be empty",
        ));
    }

    for part in value.split('.') {
        if part.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "package/group cannot contain empty segments",
            ));
        }
        let mut chars = part.chars();
        let Some(first) = chars.next() else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "package/group segment cannot be empty",
            ));
        };
        if !(first.is_ascii_alphabetic() || first == '_') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("invalid package/group segment `{part}`"),
            ));
        }
        if !chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("invalid package/group segment `{part}`"),
            ));
        }
    }

    Ok(())
}

fn render_template(
    template: TemplateKind,
    project_name: &str,
    group: &str,
    package_name: &str,
) -> Result<Vec<(PathBuf, String)>, io::Error> {
    let files = match template {
        TemplateKind::JavaMinimal => render_java_minimal(project_name, group, package_name),
        TemplateKind::JavaLib => render_java_lib(project_name, group, package_name),
        TemplateKind::JavaCli => render_java_cli(project_name, group, package_name),
        TemplateKind::JavaServer => render_java_server(project_name, group, package_name),
        TemplateKind::JavaWorkspace => render_java_workspace(project_name, group, package_name)?,
    };
    Ok(files)
}

fn render_java_minimal(
    project_name: &str,
    _group: &str,
    package_name: &str,
) -> Vec<(PathBuf, String)> {
    let package_path = package_path(package_name);
    vec![
        (
            PathBuf::from("jot.toml"),
            format!(
                "[project]\nname = \"{project_name}\"\nversion = \"0.1.0\"\nmain-class = \"{package_name}.Main\"\n\n[toolchains]\njava = \"21\"\n\n[test-dependencies]\njunit = \"org.junit.jupiter:junit-jupiter:5.11.0\"\n",
            ),
        ),
        (
            PathBuf::from(".gitignore"),
            "target/\n.jot.lock\n".to_owned(),
        ),
        (
            PathBuf::from("README.md"),
            format!(
                "# {project_name}\n\nMinimal single-project Java app for jot.\n\n## Commands\n\n```bash\njot build\njot test\njot run -- --name jot\n```\n"
            ),
        ),
        (
            PathBuf::from(format!("src/main/java/{package_path}/Main.java")),
            format!(
                "package {package_name};\n\npublic final class Main {{\n    public static void main(String[] args) {{\n        String name = args.length > 0 ? args[0] : \"world\";\n        System.out.println(\"hello \" + name);\n    }}\n}}\n"
            ),
        ),
        (
            PathBuf::from(format!("src/test/java/{package_path}/MainTest.java")),
            format!(
                "package {package_name};\n\nimport org.junit.jupiter.api.Test;\n\nimport static org.junit.jupiter.api.Assertions.assertTrue;\n\nclass MainTest {{\n    @Test\n    void sanityCheck() {{\n        assertTrue(true);\n    }}\n}}\n"
            ),
        ),
    ]
}

fn render_java_lib(project_name: &str, _group: &str, package_name: &str) -> Vec<(PathBuf, String)> {
    let package_path = package_path(package_name);
    vec![
        (
            PathBuf::from("jot.toml"),
            format!(
                "[project]\nname = \"{project_name}\"\nversion = \"0.1.0\"\n\n[toolchains]\njava = \"21\"\n\n[dependencies]\njackson = \"com.fasterxml.jackson.core:jackson-databind:2.18.0\"\n\n[test-dependencies]\njunit = \"org.junit.jupiter:junit-jupiter:5.11.0\"\n",
            ),
        ),
        (
            PathBuf::from("README.md"),
            format!(
                "# {project_name}\n\nReusable Java library sample for jot.\n\n## Commands\n\n```bash\njot build\njot test\n```\n"
            ),
        ),
        (
            PathBuf::from(".gitignore"),
            "target/\n.jot.lock\n".to_owned(),
        ),
        (
            PathBuf::from(format!("src/main/java/{package_path}/GreetingService.java")),
            format!(
                "package {package_name};\n\npublic final class GreetingService {{\n    public String greetingFor(String value) {{\n        return \"hello \" + value;\n    }}\n}}\n"
            ),
        ),
        (
            PathBuf::from(format!(
                "src/test/java/{package_path}/GreetingServiceTest.java"
            )),
            format!(
                "package {package_name};\n\nimport org.junit.jupiter.api.Test;\n\nimport static org.junit.jupiter.api.Assertions.assertEquals;\n\nclass GreetingServiceTest {{\n    @Test\n    void buildsExpectedGreeting() {{\n        GreetingService service = new GreetingService();\n        assertEquals(\"hello jot\", service.greetingFor(\"jot\"));\n    }}\n}}\n"
            ),
        ),
    ]
}

fn render_java_cli(project_name: &str, _group: &str, package_name: &str) -> Vec<(PathBuf, String)> {
    let package_path = package_path(package_name);
    vec![
        (
            PathBuf::from("jot.toml"),
            format!(
                "[project]\nname = \"{project_name}\"\nversion = \"0.1.0\"\nmain-class = \"{package_name}.CliMain\"\n\n[toolchains]\njava = \"21\"\n\n[dependencies]\npicocli = \"info.picocli:picocli:4.7.6\"\n\n[test-dependencies]\njunit = \"org.junit.jupiter:junit-jupiter:5.11.0\"\n",
            ),
        ),
        (
            PathBuf::from("README.md"),
            format!(
                "# {project_name}\n\nCLI sample for jot using Picocli.\n\n## Commands\n\n```bash\njot build\njot test\njot run -- --help\njot run -- Ada\n```\n"
            ),
        ),
        (
            PathBuf::from(".gitignore"),
            "target/\n.jot.lock\n".to_owned(),
        ),
        (
            PathBuf::from(format!("src/main/java/{package_path}/CliMain.java")),
            format!(
                "package {package_name};\n\nimport picocli.CommandLine;\nimport picocli.CommandLine.Command;\nimport picocli.CommandLine.Parameters;\n\n@Command(name = \"{project_name}\", mixinStandardHelpOptions = true, description = \"Greets the requested user.\")\npublic final class CliMain implements Runnable {{\n    @Parameters(index = \"0\", defaultValue = \"jot\", description = \"Name to greet.\")\n    private String name;\n\n    public static void main(String[] args) {{\n        int exitCode = new CommandLine(new CliMain()).execute(args);\n        System.exit(exitCode);\n    }}\n\n    @Override\n    public void run() {{\n        System.out.println(\"hello from cli, \" + name);\n    }}\n}}\n"
            ),
        ),
        (
            PathBuf::from(format!("src/test/java/{package_path}/CliMainTest.java")),
            format!(
                "package {package_name};\n\nimport org.junit.jupiter.api.Test;\n\nimport static org.junit.jupiter.api.Assertions.assertTrue;\n\nclass CliMainTest {{\n    @Test\n    void sanityCheck() {{\n        assertTrue(true);\n    }}\n}}\n"
            ),
        ),
    ]
}

fn render_java_server(
    project_name: &str,
    _group: &str,
    package_name: &str,
) -> Vec<(PathBuf, String)> {
    let package_path = package_path(package_name);
    vec![
        (
            PathBuf::from("jot.toml"),
            format!(
                "[project]\nname = \"{project_name}\"\nversion = \"0.1.0\"\nmain-class = \"{package_name}.ServerMain\"\n\n[toolchains]\njava = \"21\"\n\n[test-dependencies]\njunit = \"org.junit.jupiter:junit-jupiter:5.11.0\"\n",
            ),
        ),
        (
            PathBuf::from("README.md"),
            format!(
                "# {project_name}\n\nHTTP server sample for jot.\n\n## Commands\n\n```bash\njot build\njot test\njot run\n```\n"
            ),
        ),
        (
            PathBuf::from(".gitignore"),
            "target/\n.jot.lock\n".to_owned(),
        ),
        (
            PathBuf::from(format!("src/main/java/{package_path}/ServerMain.java")),
            format!(
                "package {package_name};\n\nimport com.sun.net.httpserver.HttpServer;\nimport java.io.IOException;\nimport java.io.OutputStream;\nimport java.net.InetSocketAddress;\n\npublic final class ServerMain {{\n    public static void main(String[] args) throws IOException {{\n        HttpServer server = HttpServer.create(new InetSocketAddress(8080), 0);\n        server.createContext(\"/health\", exchange -> {{\n            byte[] body = \"ok\".getBytes();\n            exchange.sendResponseHeaders(200, body.length);\n            try (OutputStream output = exchange.getResponseBody()) {{\n                output.write(body);\n            }}\n        }});\n        server.start();\n        System.out.println(\"server started on http://localhost:8080/health\");\n    }}\n}}\n"
            ),
        ),
        (
            PathBuf::from(format!("src/test/java/{package_path}/ServerMainTest.java")),
            format!(
                "package {package_name};\n\nimport org.junit.jupiter.api.Test;\n\nimport static org.junit.jupiter.api.Assertions.assertEquals;\n\nclass ServerMainTest {{\n    @Test\n    void healthPayload() {{\n        assertEquals(\"ok\", \"ok\");\n    }}\n}}\n"
            ),
        ),
    ]
}

fn render_java_workspace(
    project_name: &str,
    group: &str,
    package_name: &str,
) -> Result<Vec<(PathBuf, String)>, io::Error> {
    let base_package = package_name.to_owned();
    validate_package_name(&base_package)?;

    let domain_package = format!("{base_package}.domain");
    let api_package = format!("{base_package}.api");
    let cli_package = format!("{base_package}.cli");

    let domain_path = package_path(&domain_package);
    let api_path = package_path(&api_package);
    let cli_path = package_path(&cli_package);

    Ok(vec![
        (
            PathBuf::from("jot.toml"),
            format!(
                "[workspace]\nmembers = [\"domain\", \"api\", \"cli\"]\ngroup = \"{group}\"\n\n[toolchains]\njava = \"21\"\n"
            ),
        ),
        (
            PathBuf::from("libs.versions.toml"),
            "[versions]\njackson = \"2.18.0\"\njunit = \"5.11.0\"\n\n[libraries]\njackson-databind = { module = \"com.fasterxml.jackson.core:jackson-databind\", version.ref = \"jackson\" }\njunit = { module = \"org.junit.jupiter:junit-jupiter\", version.ref = \"junit\" }\n".to_owned(),
        ),
        (
            PathBuf::from("README.md"),
            format!(
                "# {project_name}\n\nMulti-module workspace example for jot.\n\nModules:\n- domain: shared library\n- api: simple web server\n- cli: command-line entrypoint\n\n## Commands\n\n```bash\njot build\njot test\njot run --module api\njot run --module cli -- --help\n```\n"
            ),
        ),
        (
            PathBuf::from("domain/jot.toml"),
            "[project]\nname = \"shopflow-domain\"\nversion = \"1.0.0\"\n\n[dependencies]\njackson = { catalog = \"jackson-databind\" }\n\n[test-dependencies]\njunit = { catalog = \"junit\" }\n"
                .to_owned(),
        ),
        (
            PathBuf::from(format!("domain/src/main/java/{domain_path}/Order.java")),
            format!(
                "package {domain_package};\n\npublic record Order(String id, String customer) {{\n}}\n"
            ),
        ),
        (
            PathBuf::from(format!("domain/src/test/java/{domain_path}/OrderTest.java")),
            format!(
                "package {domain_package};\n\nimport org.junit.jupiter.api.Test;\n\nimport static org.junit.jupiter.api.Assertions.assertEquals;\n\nclass OrderTest {{\n    @Test\n    void exposesOrderData() {{\n        Order order = new Order(\"A-1\", \"jot\");\n        assertEquals(\"A-1\", order.id());\n    }}\n}}\n"
            ),
        ),
        (
            PathBuf::from("api/jot.toml"),
            format!(
                "[project]\nname = \"shopflow-api\"\nversion = \"1.0.0\"\nmain-class = \"{api_package}.ApiMain\"\n\n[dependencies]\ndomain = {{ path = \"../domain\" }}\n\n[test-dependencies]\njunit = {{ catalog = \"junit\" }}\n"
            ),
        ),
        (
            PathBuf::from(format!("api/src/main/java/{api_path}/ApiMain.java")),
            format!(
                "package {api_package};\n\nimport {domain_package}.Order;\nimport com.sun.net.httpserver.HttpServer;\nimport java.io.IOException;\nimport java.io.OutputStream;\nimport java.net.InetSocketAddress;\n\npublic final class ApiMain {{\n    public static void main(String[] args) throws IOException {{\n        HttpServer server = HttpServer.create(new InetSocketAddress(8080), 0);\n        server.createContext(\"/health\", exchange -> {{\n            Order order = new Order(\"A-1\", \"jot\");\n            byte[] body = (\"ok:\" + order.id()).getBytes();\n            exchange.sendResponseHeaders(200, body.length);\n            try (OutputStream output = exchange.getResponseBody()) {{\n                output.write(body);\n            }}\n        }});\n        server.start();\n        System.out.println(\"shopflow api listening on :8080\");\n    }}\n}}\n"
            ),
        ),
        (
            PathBuf::from(format!("api/src/test/java/{api_path}/ApiMainTest.java")),
            format!(
                "package {api_package};\n\nimport org.junit.jupiter.api.Test;\n\nimport static org.junit.jupiter.api.Assertions.assertTrue;\n\nclass ApiMainTest {{\n    @Test\n    void sanityCheck() {{\n        assertTrue(true);\n    }}\n}}\n"
            ),
        ),
        (
            PathBuf::from("cli/jot.toml"),
            format!(
                "[project]\nname = \"shopflow-cli\"\nversion = \"1.0.0\"\nmain-class = \"{cli_package}.CliMain\"\n\n[dependencies]\ndomain = {{ path = \"../domain\" }}\n\n[test-dependencies]\njunit = {{ catalog = \"junit\" }}\n"
            ),
        ),
        (
            PathBuf::from(format!("cli/src/main/java/{cli_path}/CliMain.java")),
            format!(
                "package {cli_package};\n\nimport {domain_package}.Order;\n\npublic final class CliMain {{\n    public static void main(String[] args) {{\n        if (args.length > 0 && \"--help\".equals(args[0])) {{\n            System.out.println(\"usage: shopflow-cli [customer]\");\n            return;\n        }}\n\n        String customer = args.length > 0 ? args[0] : \"jot\";\n        Order order = new Order(\"A-1\", customer);\n        System.out.println(\"generated order for \" + order.customer());\n    }}\n}}\n"
            ),
        ),
        (
            PathBuf::from(format!("cli/src/test/java/{cli_path}/CliMainTest.java")),
            format!(
                "package {cli_package};\n\nimport org.junit.jupiter.api.Test;\n\nimport static org.junit.jupiter.api.Assertions.assertEquals;\n\nclass CliMainTest {{\n    @Test\n    void simpleMath() {{\n        assertEquals(4, 2 + 2);\n    }}\n}}\n"
            ),
        ),
        (
            PathBuf::from(".gitignore"),
            "target/\n**/target/\n.jot.lock\n".to_owned(),
        ),
    ])
}

fn package_path(package_name: &str) -> String {
    package_name.replace('.', "/")
}

impl TemplateKind {
    fn name(self) -> &'static str {
        match self {
            TemplateKind::JavaMinimal => "java-minimal",
            TemplateKind::JavaLib => "java-lib",
            TemplateKind::JavaCli => "java-cli",
            TemplateKind::JavaServer => "java-server",
            TemplateKind::JavaWorkspace => "java-workspace",
        }
    }
}
