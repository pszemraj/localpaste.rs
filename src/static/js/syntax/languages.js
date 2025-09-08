/**
 * Language pattern definitions for syntax highlighting
 */
export const languages = {
    python: {
        patterns: [
            { regex: /#.*$/gm, type: 'comment' },
            { regex: /"""[\s\S]*?"""|'''[\s\S]*?'''/g, type: 'string' },
            { regex: /(['"])(?:[^\\]|\\.)*?\1/g, type: 'string' },
            { regex: /\b(?:def|class|import|from|as|if|elif|else|for|while|break|continue|return|try|except|finally|with|lambda|pass|raise|yield|assert|del|global|nonlocal|in|is|not|and|or|True|False|None|self|async|await)\b/g, type: 'keyword' },
            { regex: /\b\d+(?:\.\d+)?\b/g, type: 'number' },
            { regex: /\b([a-zA-Z_]\w*)\s*(?=\()/g, type: 'function', group: 1 }
        ]
    },
    javascript: {
        patterns: [
            { regex: /\/\/.*$/gm, type: 'comment' },
            { regex: /\/\*[\s\S]*?\*\//g, type: 'comment' },
            { regex: /(['"`])(?:[^\\]|\\.)*?\1/g, type: 'string' },
            { regex: /\b(?:const|let|var|function|class|if|else|for|while|do|switch|case|break|continue|return|try|catch|finally|throw|async|await|import|export|from|default|new|this|super|extends|static|typeof|instanceof|in|of|delete|void|yield|null|undefined|true|false)\b/g, type: 'keyword' },
            { regex: /\b\d+(?:\.\d+)?\b/g, type: 'number' },
            { regex: /\b([a-zA-Z_$][\w$]*)\s*(?=\()/g, type: 'function', group: 1 }
        ]
    },
    shell: {
        patterns: [
            { regex: /#.*$/gm, type: 'comment' },
            { regex: /(['"])(?:[^\\]|\\.)*?\1/g, type: 'string' },
            { regex: /\$[a-zA-Z_]\w*|\${[^}]+}/g, type: 'property' },
            { regex: /\b(?:if|then|else|elif|fi|for|while|do|done|case|esac|function|return|break|continue|exit|export|source|alias|echo|cd|ls|cp|mv|rm|mkdir|touch|cat|grep|sed|awk|find|chmod|chown|sudo|apt|yum|npm|pip|git|docker|kubectl)\b/g, type: 'keyword' }
        ]
    },
    markdown: {
        patterns: [
            { regex: /```[\s\S]*?```/g, type: 'code-block', special: true },
            { regex: /^#{1,6}\s+.*$/gm, type: 'heading' },
            { regex: /\*\*[^*]+\*\*/g, type: 'bold' },
            { regex: /__[^_]+__/g, type: 'bold' },
            { regex: /\*[^*\s][^*]*\*/g, type: 'italic' },
            { regex: /_[^_\s][^_]*_/g, type: 'italic' },
            { regex: /`[^`]+`/g, type: 'code' },
            { regex: /\[[^\]]+\]\([^)]+\)/g, type: 'link' },
            { regex: /^\s*[*+-]\s/gm, type: 'list-marker' },
            { regex: /^\s*\d+\.\s/gm, type: 'list-marker' },
            { regex: /^>\s.*$/gm, type: 'blockquote' }
        ]
    },
    cpp: {
        patterns: [
            { regex: /\/\/.*$/gm, type: 'comment' },
            { regex: /\/\*[\s\S]*?\*\//g, type: 'comment' },
            { regex: /(['"])(?:[^\\]|\\.)*?\1/g, type: 'string' },
            { regex: /\b(?:class|public|private|protected|virtual|override|namespace|using|if|else|for|while|do|switch|case|break|continue|return|try|catch|throw|new|delete|this|nullptr|const|static|inline|typedef|typename|template|auto|void|int|char|bool|float|double|long|short|unsigned|signed|struct|enum|union)\b/g, type: 'keyword' },
            { regex: /^#\s*\w+/gm, type: 'type' },
            { regex: /\b\d+(?:\.\d+)?[fFlL]?\b/g, type: 'number' },
            { regex: /\b(?:std::\w+|string|vector|map|set|cout|cin|endl)\b/g, type: 'type' }
        ]
    },
    c: {
        patterns: [
            { regex: /\/\/.*$/gm, type: 'comment' },
            { regex: /\/\*[\s\S]*?\*\//g, type: 'comment' },
            { regex: /(['"])(?:[^\\]|\\.)*?\1/g, type: 'string' },
            { regex: /\b(?:if|else|for|while|do|switch|case|break|continue|return|goto|typedef|struct|enum|union|const|static|extern|inline|sizeof|void|int|char|float|double|long|short|unsigned|signed|NULL)\b/g, type: 'keyword' },
            { regex: /^#\s*\w+/gm, type: 'type' },
            { regex: /\b\d+(?:\.\d+)?[fFlL]?\b/g, type: 'number' },
            { regex: /\b([a-zA-Z_]\w*)\s*(?=\()/g, type: 'function', group: 1 }
        ]
    },
    java: {
        patterns: [
            { regex: /\/\/.*$/gm, type: 'comment' },
            { regex: /\/\*[\s\S]*?\*\//g, type: 'comment' },
            { regex: /(['"])(?:[^\\]|\\.)*?\1/g, type: 'string' },
            { regex: /\b(?:public|private|protected|class|interface|extends|implements|abstract|final|static|void|int|char|boolean|float|double|long|short|byte|if|else|for|while|do|switch|case|break|continue|return|try|catch|finally|throw|throws|new|this|super|import|package|null|true|false)\b/g, type: 'keyword' },
            { regex: /@\w+/g, type: 'type' },
            { regex: /\b\d+(?:\.\d+)?[fFlLdD]?\b/g, type: 'number' },
            { regex: /\b(?:String|Integer|Double|Float|Boolean|ArrayList|HashMap|List|Map|Set)\b/g, type: 'type' }
        ]
    },
    csharp: {
        patterns: [
            { regex: /\/\/.*$/gm, type: 'comment' },
            { regex: /\/\*[\s\S]*?\*\//g, type: 'comment' },
            { regex: /(['"])(?:[^\\]|\\.)*?\1/g, type: 'string' },
            { regex: /\$"[^"]*"/g, type: 'string' },
            { regex: /\b(?:public|private|protected|internal|class|interface|struct|enum|abstract|sealed|static|void|int|char|bool|float|double|decimal|long|short|byte|string|if|else|for|foreach|while|do|switch|case|break|continue|return|try|catch|finally|throw|using|namespace|new|this|base|null|true|false|var|async|await|override|virtual|const|readonly)\b/g, type: 'keyword' },
            { regex: /\b\d+(?:\.\d+)?[fFmMdD]?\b/g, type: 'number' },
            { regex: /\b(?:String|Int32|Double|Float|Boolean|List|Dictionary|Task)\b/g, type: 'type' }
        ]
    },
    go: {
        patterns: [
            { regex: /\/\/.*$/gm, type: 'comment' },
            { regex: /\/\*[\s\S]*?\*\//g, type: 'comment' },
            { regex: /(['"`])(?:[^\\]|\\.)*?\1/g, type: 'string' },
            { regex: /\b(?:package|import|func|var|const|type|struct|interface|map|chan|if|else|for|range|switch|case|break|continue|return|go|defer|select|fallthrough|default|nil|true|false)\b/g, type: 'keyword' },
            { regex: /\b(?:bool|string|int|int8|int16|int32|int64|uint|uint8|uint16|uint32|uint64|float32|float64|complex64|complex128|byte|rune|error)\b/g, type: 'type' },
            { regex: /\b\d+(?:\.\d+)?\b/g, type: 'number' },
            { regex: /\b([a-zA-Z_]\w*)\s*(?=\()/g, type: 'function', group: 1 }
        ]
    },
    rust: {
        patterns: [
            { regex: /\/\/.*$/gm, type: 'comment' },
            { regex: /\/\*[\s\S]*?\*\//g, type: 'comment' },
            { regex: /(['"])(?:[^\\]|\\.)*?\1/g, type: 'string' },
            { regex: /\b(?:fn|let|mut|const|if|else|match|for|while|loop|break|continue|return|use|mod|pub|struct|enum|trait|impl|self|Self|super|crate|async|await|move|ref|dyn|static|extern|unsafe|where|as|in|type)\b/g, type: 'keyword' },
            { regex: /\b(?:bool|char|i8|i16|i32|i64|i128|isize|u8|u16|u32|u64|u128|usize|f32|f64|str|String|Vec|Option|Result|Box|Rc|Arc|HashMap|HashSet)\b/g, type: 'type' },
            { regex: /'[a-z]\w*/g, type: 'type' },
            { regex: /\b\d+(?:\.\d+)?(?:[iu](?:8|16|32|64|128|size))?\b/g, type: 'number' },
            { regex: /\b([a-zA-Z_]\w*)!/g, type: 'function', group: 1 }
        ]
    },
    html: {
        patterns: [
            { regex: /<!--[\s\S]*?-->/g, type: 'comment' },
            { regex: /<(\/?[a-zA-Z][\w-]*)/g, type: 'tag', group: 1 },
            { regex: /\s([a-zA-Z][\w-]*)=/g, type: 'attribute', group: 1 },
            { regex: /(['"])(?:[^\\]|\\.)*?\1/g, type: 'string' }
        ]
    },
    xml: {
        patterns: [
            { regex: /<!--[\s\S]*?-->/g, type: 'comment' },
            { regex: /<(\/?[a-zA-Z][\w-]*)/g, type: 'tag', group: 1 },
            { regex: /\s([a-zA-Z][\w-]*)=/g, type: 'attribute', group: 1 },
            { regex: /(['"])(?:[^\\]|\\.)*?\1/g, type: 'string' }
        ]
    },
    css: {
        patterns: [
            { regex: /\/\*[\s\S]*?\*\//g, type: 'comment' },
            { regex: /([.#]?[\w-:]+)(?=\s*\{)/g, type: 'tag' },
            { regex: /([\w-]+)(?=\s*:)/g, type: 'property' },
            { regex: /#[0-9a-fA-F]{3,8}\b/g, type: 'number' },
            { regex: /\b\d+(?:\.\d+)?(?:px|em|rem|%|vh|vw|deg|s|ms)?\b/g, type: 'number' },
            { regex: /(['"])(?:[^\\]|\\.)*?\1/g, type: 'string' },
            { regex: /!important/g, type: 'keyword' }
        ]
    },
    json: {
        patterns: [
            { regex: /"([^"]+)"\s*:/g, type: 'property', group: 1, wrap: '"' },
            { regex: /:\s*"([^"]*)"/g, type: 'string', group: 1, wrap: '"' },
            { regex: /\b-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?\b/g, type: 'number' },
            { regex: /\b(?:true|false|null)\b/g, type: 'keyword' }
        ]
    },
    yaml: {
        patterns: [
            { regex: /#.*$/gm, type: 'comment' },
            { regex: /^(\s*)([\w-]+):/gm, type: 'property', group: 2 },
            { regex: /(['"])(?:[^\\]|\\.)*?\1/g, type: 'string' },
            { regex: /\b(?:true|false|yes|no|null)\b/g, type: 'keyword' },
            { regex: /\b\d+(?:\.\d+)?\b/g, type: 'number' }
        ]
    },
    toml: {
        patterns: [
            { regex: /#.*$/gm, type: 'comment' },
            { regex: /^\[([\w.]+)\]/gm, type: 'type', group: 1 },
            { regex: /^\[\[([\w.]+)\]\]/gm, type: 'type', group: 1 },
            { regex: /^(\s*)([\w-]+)\s*=/gm, type: 'property', group: 2 },
            { regex: /(['"])(?:[^\\]|\\.)*?\1/g, type: 'string' },
            { regex: /\b(?:true|false)\b/g, type: 'keyword' },
            { regex: /\b-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?\b/g, type: 'number' },
            { regex: /\d{4}-\d{2}-\d{2}(?:T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})?)?/g, type: 'number' }
        ]
    },
    latex: {
        patterns: [
            { regex: /%.*$/gm, type: 'comment' },
            { regex: /\\[a-zA-Z]+/g, type: 'keyword' },
            { regex: /\$[^$]+\$|\$\$[\s\S]*?\$\$/g, type: 'string' },
            { regex: /\\begin\{(\w+)\}/g, type: 'type', group: 1 },
            { regex: /\\end\{(\w+)\}/g, type: 'type', group: 1 },
            { regex: /[{}]/g, type: 'operator' }
        ]
    },
    plaintext: {
        patterns: []
    }
};