#!/usr/bin/env python3
"""Generates the bundled Material Icon Theme data used for file and folder
icons in archive previews.

Usage:
    npm pack material-icon-theme && tar xzf material-icon-theme-*.tgz
    python3 scripts/gen_material_icons.py package

Writes:
    src/assets/material-icons.bin       every icon's SVG, concatenated and
                                        zlib-compressed
    src/core/material_icons_data.rs     the icon index and the extension,
                                        file name, and folder name tables

Icons come from https://github.com/material-extensions/vscode-material-icon-theme
(MIT license, see src/assets/material-icons-LICENSE).
"""

import json
import os
import shutil
import sys
import zlib

# VS Code picks an icon by language for files whose extension isn't listed
# in the theme itself (.rs, .py, .ts, ...); these are the extensions VS Code
# assigns to each built-in language id.
LANGUAGE_EXTENSIONS = {
    "bat": ["bat", "cmd"],
    "c": ["c", "i"],
    "clojure": ["clj", "cljs", "cljc", "cljx", "clojure", "edn"],
    "coffeescript": ["coffee", "cson", "iced"],
    "cpp": ["cpp", "cc", "cxx", "c++", "hpp", "hh", "hxx", "h++", "ino", "inl", "ipp", "tpp", "txx"],
    "csharp": ["cs", "csx", "cake"],
    "css": ["css"],
    "dart": ["dart"],
    "diff": ["diff", "patch", "rej"],
    "dockerfile": ["dockerfile", "containerfile"],
    "fsharp": ["fs", "fsi", "fsx", "fsscript"],
    "go": ["go"],
    "groovy": ["groovy", "gvy", "gradle", "jenkinsfile", "nf"],
    "handlebars": ["handlebars", "hbs", "hjs"],
    "hlsl": ["hlsl", "hlsli", "fx", "fxh", "vsh", "psh", "cginc", "compute"],
    "html": ["html", "htm", "shtml", "xhtml", "xht", "mdoc", "jsp", "asp", "aspx", "jshtm", "volt", "ejs", "rhtml"],
    "ini": ["ini"],
    "java": ["java", "jav"],
    "javascript": ["js", "es6", "mjs", "cjs", "pac"],
    "javascriptreact": ["jsx"],
    "json": ["json", "bowerrc", "jscsrc", "webmanifest", "js.map", "css.map", "ts.map", "har", "jslintrc", "jsonld", "geojson", "ipynb", "vuerc"],
    "jsonc": ["jsonc", "eslintrc", "eslintrc.json", "jsfmtrc", "jshintrc", "swcrc", "hintrc", "babelrc", "code-workspace", "language-configuration.json", "code-snippets"],
    "julia": ["jl"],
    "latex": ["tex", "ltx", "ctx"],
    "less": ["less"],
    "lua": ["lua"],
    "makefile": ["mak", "mk"],
    "markdown": ["md", "mkd", "mdwn", "mdown", "markdown", "markdn", "mdtxt", "mdtext", "workbook"],
    "objective-c": ["m"],
    "objective-cpp": ["mm"],
    "perl": ["pl", "pm", "pod", "t", "psgi"],
    "php": ["php", "php4", "php5", "phtml", "ctp"],
    "plaintext": ["txt"],
    "powershell": ["ps1", "psm1", "psd1", "pssc", "psrc"],
    "properties": ["properties", "cfg", "conf", "directory", "gitattributes", "gitconfig", "gitmodules", "editorconfig", "repo"],
    "pug": ["pug", "jade"],
    "python": ["py", "rpy", "pyw", "cpy", "gyp", "gypi", "pyi", "ipy", "pyt"],
    "r": ["r", "rhistory", "rprofile", "rt"],
    "razor": ["cshtml", "razor"],
    "ruby": ["rb", "rbx", "rjs", "gemspec", "rake", "ru", "erb", "podspec", "rbi"],
    "rust": ["rs"],
    "scss": ["scss"],
    "shaderlab": ["shader"],
    "shellscript": ["sh", "bash", "bashrc", "bash_aliases", "bash_profile", "bash_login", "ebuild", "eclass", "profile", "bash_logout", "xprofile", "xsession", "xsessionrc", "zsh", "zshrc", "zprofile", "zlogin", "zlogout", "zshenv", "zsh-theme", "fish", "ksh", "csh", "cshrc", "tcshrc", "yashrc", "yash_profile"],
    "sql": ["sql", "dsql"],
    "swift": ["swift"],
    "typescript": ["ts", "cts", "mts"],
    "typescriptreact": ["tsx"],
    "vb": ["vb", "brs", "vbs", "bas", "vba"],
    "xml": ["xml", "xsd", "ascx", "atom", "axml", "axaml", "bpmn", "cpt", "csl", "csproj", "dita", "ditamap", "dtd", "ent", "mod", "dtml", "fsproj", "fxml", "iml", "isml", "jmx", "launch", "menu", "mxml", "nuspec", "opml", "owl", "proj", "props", "pt", "publishsettings", "pubxml", "rdf", "rng", "rss", "shproj", "storyboard", "svg", "targets", "tld", "tmx", "vbproj", "vcxproj", "wsdl", "wxi", "wxl", "wxs", "xaml", "xbl", "xib", "xlf", "xliff", "xpdl", "xul", "xoml", "xsl", "xslt"],
    "yaml": ["yml", "eyaml", "eyml", "yaml", "cff"],
}


def rust_str(s):
    return json.dumps(s, ensure_ascii=False)


def main():
    pkg = sys.argv[1]
    repo = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    theme = json.load(open(os.path.join(pkg, "dist", "material-icons.json"), encoding="utf-8"))
    defs = theme["iconDefinitions"]

    def svg_path(icon):
        d = defs.get(icon)
        if not d:
            return None
        path = os.path.normpath(os.path.join(pkg, "dist", d["iconPath"]))
        return path if os.path.exists(path) else None

    def table(mapping):
        # Entries whose icon isn't shipped as a file (the theme generates
        # some "clone" icons when the extension is built) are left out, so
        # lookup falls through to the next rule.
        return {k.lower(): v for k, v in mapping.items() if svg_path(v)}

    extensions = {}
    for language, exts in LANGUAGE_EXTENSIONS.items():
        icon = theme["languageIds"].get(language)
        if icon and svg_path(icon):
            for ext in exts:
                extensions[ext] = icon
    extensions.update(table(theme["fileExtensions"]))  # the theme's own list wins
    file_names = table(theme["fileNames"])
    folders = table(theme["folderNames"])
    folders_open = table(theme["folderNamesExpanded"])
    light = theme.get("light", {})
    light_pairs = {}
    for key in ("fileExtensions", "fileNames", "folderNames", "folderNamesExpanded", "languageIds"):
        for name, light_icon in light.get(key, {}).items():
            source = {
                "fileExtensions": theme["fileExtensions"],
                "fileNames": theme["fileNames"],
                "folderNames": theme["folderNames"],
                "folderNamesExpanded": theme["folderNamesExpanded"],
                "languageIds": theme["languageIds"],
            }[key]
            dark_icon = source.get(name)
            if dark_icon and svg_path(dark_icon) and svg_path(light_icon) and dark_icon != light_icon:
                light_pairs[dark_icon] = light_icon

    used = set(extensions.values()) | set(file_names.values()) | set(folders.values())
    used |= set(folders_open.values()) | {"file", "folder", "folder-open"}
    light_pairs = {dark: lit for dark, lit in light_pairs.items() if dark in used}
    used |= set(light_pairs.values())
    icons = sorted(used)
    index = {name: i for i, name in enumerate(icons)}

    blob = bytearray()
    spans = []
    for name in icons:
        data = open(svg_path(name), "rb").read()
        spans.append((len(blob), len(data)))
        blob += data
    compressed = zlib.compress(bytes(blob), 9)
    with open(os.path.join(repo, "src", "assets", "material-icons.bin"), "wb") as f:
        f.write(compressed)
    shutil.copyfile(os.path.join(pkg, "LICENSE"), os.path.join(repo, "src", "assets", "material-icons-LICENSE"))

    version = json.load(open(os.path.join(pkg, "package.json"), encoding="utf-8"))["version"]
    out = []
    out.append(f"// Generated by scripts/gen_material_icons.py from material-icon-theme {version}. Don't edit.\n")
    out.append("#![cfg_attr(rustfmt, rustfmt_skip)]\n\n")
    out.append(f"pub const UNCOMPRESSED_LEN: usize = {len(blob)};\n\n")
    out.append("/// Icon name, byte offset, and length in the uncompressed blob, by name.\n")
    out.append("pub static ICONS: &[(&str, u32, u32)] = &[\n")
    for name, (offset, length) in zip(icons, spans):
        out.append(f"({rust_str(name)},{offset},{length}),\n")
    out.append("];\n\n")

    def write_map(const, doc, mapping):
        out.append(f"/// {doc}\n")
        out.append(f"pub static {const}: &[(&str, u16)] = &[\n")
        for key in sorted(mapping):
            out.append(f"({rust_str(key)},{index[mapping[key]]}),\n")
        out.append("];\n\n")

    write_map("EXTENSIONS", "Lowercase extension (without the dot, may hold dots) -> icon.", extensions)
    write_map("FILE_NAMES", "Lowercase file name -> icon.", file_names)
    write_map("FOLDERS", "Lowercase folder name -> closed folder icon.", folders)
    write_map("FOLDERS_OPEN", "Lowercase folder name -> open folder icon.", folders_open)
    out.append("/// Dark-theme icon -> its light-theme variant.\n")
    out.append("pub static LIGHT: &[(u16, u16)] = &[\n")
    for dark in sorted(light_pairs, key=lambda n: index[n]):
        out.append(f"({index[dark]},{index[light_pairs[dark]]}),\n")
    out.append("];\n")
    with open(os.path.join(repo, "src", "core", "material_icons_data.rs"), "w", encoding="utf-8") as f:
        f.write("".join(out))
    print(f"{len(icons)} icons, {len(blob)} bytes -> {len(compressed)} compressed; "
          f"{len(extensions)} extensions, {len(file_names)} file names, {len(folders)} folders")


if __name__ == "__main__":
    main()
