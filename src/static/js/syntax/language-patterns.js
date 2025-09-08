// Language pattern definitions for syntax highlighting
export const languages = {
    javascript: {
        patterns: [
            { regex: /\/\/.*$/gm, class: 'comment' },
            { regex: /\/\*[\s\S]*?\*\//g, class: 'comment' },
            { regex: /(["'`])(?:(?=(\\?))\2.)*?\1/g, class: 'string' },
            { regex: /\b(const|let|var|function|return|if|else|for|while|do|switch|case|break|continue|try|catch|finally|throw|new|typeof|instanceof|in|of|async|await|yield|import|export|from|default|class|extends|static|get|set)\b/g, class: 'keyword' },
            { regex: /\b(true|false|null|undefined|NaN|Infinity)\b/g, class: 'literal' },
            { regex: /\b\d+\.?\d*\b/g, class: 'number' },
            { regex: /\b([A-Z][a-zA-Z0-9_]*)\b/g, class: 'class' }
        ]
    },
    python: {
        patterns: [
            { regex: /#.*$/gm, class: 'comment' },
            { regex: /"""[\s\S]*?"""|'''[\s\S]*?'''/g, class: 'string' },
            { regex: /(["'])(?:(?=(\\?))\2.)*?\1/g, class: 'string' },
            { regex: /\b(def|class|if|elif|else|for|while|return|import|from|as|try|except|finally|with|lambda|yield|pass|break|continue|and|or|not|is|in|True|False|None|self|async|await)\b/g, class: 'keyword' },
            { regex: /\b\d+\.?\d*\b/g, class: 'number' },
            { regex: /\b([A-Z][a-zA-Z0-9_]*)\b/g, class: 'class' }
        ]
    },
    rust: {
        patterns: [
            { regex: /\/\/.*$/gm, class: 'comment' },
            { regex: /\/\*[\s\S]*?\*\//g, class: 'comment' },
            { regex: /"(?:[^"\\]|\\.)*"/g, class: 'string' },
            { regex: /\b(fn|let|mut|const|if|else|match|for|while|loop|return|use|mod|pub|struct|enum|impl|trait|type|where|async|await|move|ref|break|continue|self|Self|super|crate|static|extern|unsafe)\b/g, class: 'keyword' },
            { regex: /\b(bool|char|i8|i16|i32|i64|i128|isize|u8|u16|u32|u64|u128|usize|f32|f64|str|String|Vec|Option|Result|Box)\b/g, class: 'type' },
            { regex: /\b\d+\.?\d*\b/g, class: 'number' },
            { regex: /\b[a-z_][a-z0-9_]*!/g, class: 'macro' }
        ]
    },
    go: {
        patterns: [
            { regex: /\/\/.*$/gm, class: 'comment' },
            { regex: /\/\*[\s\S]*?\*\//g, class: 'comment' },
            { regex: /(["'`])(?:(?=(\\?))\2.)*?\1/g, class: 'string' },
            { regex: /\b(package|import|func|var|const|type|struct|interface|if|else|switch|case|default|for|range|return|break|continue|goto|defer|go|select|chan|map|nil)\b/g, class: 'keyword' },
            { regex: /\b(bool|byte|complex64|complex128|error|float32|float64|int|int8|int16|int32|int64|rune|string|uint|uint8|uint16|uint32|uint64|uintptr)\b/g, class: 'type' },
            { regex: /\b\d+\.?\d*\b/g, class: 'number' }
        ]
    },
    html: {
        patterns: [
            { regex: /<!--[\s\S]*?-->/g, class: 'comment' },
            { regex: /<\/?[a-zA-Z][^>]*>/g, class: 'tag' },
            { regex: /=["']([^"']*?)["']/g, class: 'string' }
        ]
    },
    css: {
        patterns: [
            { regex: /\/\*[\s\S]*?\*\//g, class: 'comment' },
            { regex: /([.#]?[a-zA-Z][\w-]*)\s*{/g, class: 'selector' },
            { regex: /([a-zA-Z-]+):/g, class: 'property' },
            { regex: /#[0-9a-fA-F]{3,6}\b/g, class: 'color' },
            { regex: /\b\d+\.?\d*(px|em|rem|%|vh|vw)?\b/g, class: 'number' }
        ]
    },
    json: {
        patterns: [
            { regex: /"([^"\\]|\\.)*":/g, class: 'property' },
            { regex: /:\s*"([^"\\]|\\.)*"/g, class: 'string' },
            { regex: /:\s*\d+\.?\d*/g, class: 'number' },
            { regex: /:\s*(true|false|null)/g, class: 'literal' }
        ]
    },
    yaml: {
        patterns: [
            { regex: /#.*$/gm, class: 'comment' },
            { regex: /^[\w-]+:/gm, class: 'property' },
            { regex: /:\s*"([^"\\]|\\.)*"/g, class: 'string' },
            { regex: /:\s*'([^'\\]|\\.)*'/g, class: 'string' },
            { regex: /:\s*\d+\.?\d*/g, class: 'number' },
            { regex: /:\s*(true|false|null|yes|no|on|off)/gi, class: 'literal' }
        ]
    },
    toml: {
        patterns: [
            { regex: /#.*$/gm, class: 'comment' },
            { regex: /^\[[\w.]+\]/gm, class: 'section' },
            { regex: /^[\w-]+\s*=/gm, class: 'property' },
            { regex: /=\s*"([^"\\]|\\.)*"/g, class: 'string' },
            { regex: /=\s*\d+\.?\d*/g, class: 'number' },
            { regex: /=\s*(true|false)/g, class: 'literal' }
        ]
    },
    markdown: {
        patterns: [
            { regex: /^#{1,6}\s+.*$/gm, class: 'heading' },
            { regex: /\*\*([^*]+)\*\*/g, class: 'bold' },
            { regex: /\*([^*]+)\*/g, class: 'italic' },
            { regex: /`([^`]+)`/g, class: 'code' },
            { regex: /```[\s\S]*?```/g, class: 'code-block' },
            { regex: /\[([^\]]+)\]\([^)]+\)/g, class: 'link' },
            { regex: /^[-*+]\s+/gm, class: 'list' }
        ]
    },
    shell: {
        patterns: [
            { regex: /#.*$/gm, class: 'comment' },
            { regex: /(["'])(?:(?=(\\?))\2.)*?\1/g, class: 'string' },
            { regex: /\$\w+|\${[^}]+}/g, class: 'variable' },
            { regex: /\b(if|then|else|elif|fi|for|while|do|done|case|esac|function|return|exit|break|continue|export|source|alias)\b/g, class: 'keyword' },
            { regex: /\b(echo|cd|ls|cp|mv|rm|mkdir|chmod|chown|grep|sed|awk|curl|wget|git|npm|yarn|docker)\b/g, class: 'builtin' }
        ]
    },
    sql: {
        patterns: [
            { regex: /--.*$/gm, class: 'comment' },
            { regex: /\/\*[\s\S]*?\*\//g, class: 'comment' },
            { regex: /(["'`])(?:(?=(\\?))\2.)*?\1/g, class: 'string' },
            { regex: /\b(SELECT|FROM|WHERE|JOIN|LEFT|RIGHT|INNER|OUTER|ON|AS|INSERT|INTO|VALUES|UPDATE|SET|DELETE|CREATE|TABLE|DROP|ALTER|ADD|COLUMN|INDEX|PRIMARY|KEY|FOREIGN|REFERENCES|ORDER|BY|GROUP|HAVING|LIMIT|OFFSET|UNION|ALL|DISTINCT|AND|OR|NOT|NULL|IS|IN|EXISTS|BETWEEN|LIKE|CASE|WHEN|THEN|ELSE|END)\b/gi, class: 'keyword' },
            { regex: /\b\d+\.?\d*\b/g, class: 'number' }
        ]
    }
};