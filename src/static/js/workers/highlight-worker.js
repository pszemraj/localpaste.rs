// Web Worker for syntax highlighting
// Offloads CPU-intensive highlighting to a background thread

// Import highlighting logic
self.importScripts('/js/syntax/languages.js');

// Simple tokenizer for syntax highlighting
class WorkerHighlighter {
    constructor() {
        this.languages = {
            javascript: this.jsRules(),
            python: this.pythonRules(),
            rust: this.rustRules(),
            html: this.htmlRules(),
            css: this.cssRules(),
            json: this.jsonRules(),
            markdown: this.markdownRules(),
            default: this.defaultRules()
        };
    }

    highlight(text, language = '') {
        const rules = this.languages[language] || this.languages.default;
        let highlighted = this.escapeHtml(text);
        
        // Apply highlighting rules
        rules.forEach(rule => {
            highlighted = highlighted.replace(rule.pattern, rule.replacement);
        });
        
        return highlighted;
    }

    escapeHtml(text) {
        const div = { textContent: text, innerHTML: '' };
        div.innerHTML = div.textContent
            .replace(/&/g, '&amp;')
            .replace(/</g, '&lt;')
            .replace(/>/g, '&gt;')
            .replace(/"/g, '&quot;')
            .replace(/'/g, '&#039;');
        return div.innerHTML;
    }

    jsRules() {
        return [
            // Keywords
            {
                pattern: /\b(const|let|var|function|return|if|else|for|while|do|switch|case|break|continue|try|catch|finally|throw|new|typeof|instanceof|in|of|async|await|yield|import|export|from|default|class|extends|static|get|set)\b/g,
                replacement: '<span class="keyword">$1</span>'
            },
            // Strings
            {
                pattern: /(['"`])(?:(?=(\\?))\2.)*?\1/g,
                replacement: '<span class="string">$&</span>'
            },
            // Comments
            {
                pattern: /(\/\/.*$|\/\*[\s\S]*?\*\/)/gm,
                replacement: '<span class="comment">$1</span>'
            },
            // Numbers
            {
                pattern: /\b(\d+\.?\d*)\b/g,
                replacement: '<span class="number">$1</span>'
            },
            // Functions
            {
                pattern: /\b([a-zA-Z_]\w*)\s*(?=\()/g,
                replacement: '<span class="function">$1</span>'
            }
        ];
    }

    pythonRules() {
        return [
            // Keywords
            {
                pattern: /\b(def|class|if|elif|else|for|while|return|import|from|as|try|except|finally|with|lambda|yield|pass|break|continue|and|or|not|is|in|True|False|None|self|async|await)\b/g,
                replacement: '<span class="keyword">$1</span>'
            },
            // Strings
            {
                pattern: /(['"])(?:(?=(\\?))\2.)*?\1/g,
                replacement: '<span class="string">$&</span>'
            },
            // Comments
            {
                pattern: /(#.*$)/gm,
                replacement: '<span class="comment">$1</span>'
            },
            // Numbers
            {
                pattern: /\b(\d+\.?\d*)\b/g,
                replacement: '<span class="number">$1</span>'
            },
            // Functions
            {
                pattern: /\b([a-zA-Z_]\w*)\s*(?=\()/g,
                replacement: '<span class="function">$1</span>'
            }
        ];
    }

    rustRules() {
        return [
            // Keywords
            {
                pattern: /\b(fn|let|mut|const|if|else|match|for|while|loop|return|use|mod|pub|struct|enum|impl|trait|type|where|async|await|move|ref|break|continue|self|Self|super|crate|static|extern|unsafe)\b/g,
                replacement: '<span class="keyword">$1</span>'
            },
            // Types
            {
                pattern: /\b(bool|char|i8|i16|i32|i64|i128|isize|u8|u16|u32|u64|u128|usize|f32|f64|str|String|Vec|Option|Result|Box|Rc|Arc)\b/g,
                replacement: '<span class="type">$1</span>'
            },
            // Strings
            {
                pattern: /("(?:[^"\\]|\\.)*")/g,
                replacement: '<span class="string">$1</span>'
            },
            // Comments
            {
                pattern: /(\/\/.*$|\/\*[\s\S]*?\*\/)/gm,
                replacement: '<span class="comment">$1</span>'
            },
            // Numbers
            {
                pattern: /\b(\d+\.?\d*)\b/g,
                replacement: '<span class="number">$1</span>'
            },
            // Macros
            {
                pattern: /\b([a-zA-Z_]\w*!)/g,
                replacement: '<span class="macro">$1</span>'
            }
        ];
    }

    htmlRules() {
        return [
            // Tags
            {
                pattern: /(&lt;\/?[a-zA-Z][a-zA-Z0-9]*(?:\s+[^&]*?)?&gt;)/g,
                replacement: '<span class="tag">$1</span>'
            },
            // Attributes
            {
                pattern: /(\w+)=/g,
                replacement: '<span class="attribute">$1</span>='
            },
            // Strings
            {
                pattern: /(['"])(?:(?=(\\?))\2.)*?\1/g,
                replacement: '<span class="string">$&</span>'
            },
            // Comments
            {
                pattern: /(&lt;!--[\s\S]*?--&gt;)/g,
                replacement: '<span class="comment">$1</span>'
            }
        ];
    }

    cssRules() {
        return [
            // Selectors
            {
                pattern: /([.#]?[a-zA-Z][a-zA-Z0-9-_]*)\s*{/g,
                replacement: '<span class="selector">$1</span>{'
            },
            // Properties
            {
                pattern: /([a-zA-Z-]+)\s*:/g,
                replacement: '<span class="property">$1</span>:'
            },
            // Values
            {
                pattern: /:\s*([^;]+);/g,
                replacement: ': <span class="value">$1</span>;'
            },
            // Comments
            {
                pattern: /(\/\*[\s\S]*?\*\/)/g,
                replacement: '<span class="comment">$1</span>'
            }
        ];
    }

    jsonRules() {
        return [
            // Keys
            {
                pattern: /"([^"]+)":/g,
                replacement: '<span class="key">"$1"</span>:'
            },
            // Strings
            {
                pattern: /: *"([^"]*)"/g,
                replacement: ': <span class="string">"$1"</span>'
            },
            // Numbers
            {
                pattern: /: *(\d+\.?\d*)/g,
                replacement: ': <span class="number">$1</span>'
            },
            // Booleans
            {
                pattern: /: *(true|false)/g,
                replacement: ': <span class="boolean">$1</span>'
            },
            // Null
            {
                pattern: /: *(null)/g,
                replacement: ': <span class="null">$1</span>'
            }
        ];
    }

    markdownRules() {
        return [
            // Headers
            {
                pattern: /^(#{1,6})\s+(.*)$/gm,
                replacement: '<span class="header">$1 $2</span>'
            },
            // Bold
            {
                pattern: /\*\*([^*]+)\*\*/g,
                replacement: '<span class="bold">**$1**</span>'
            },
            // Italic
            {
                pattern: /\*([^*]+)\*/g,
                replacement: '<span class="italic">*$1*</span>'
            },
            // Code blocks
            {
                pattern: /```[\s\S]*?```/g,
                replacement: '<span class="code-block">$&</span>'
            },
            // Inline code
            {
                pattern: /`([^`]+)`/g,
                replacement: '<span class="code">$&</span>'
            },
            // Links
            {
                pattern: /\[([^\]]+)\]\(([^)]+)\)/g,
                replacement: '<span class="link">[$1]($2)</span>'
            },
            // Lists
            {
                pattern: /^(\s*[-*+]\s+)/gm,
                replacement: '<span class="list">$1</span>'
            }
        ];
    }

    defaultRules() {
        return [
            // Comments (generic)
            {
                pattern: /(\/\/.*$|#.*$|\/\*[\s\S]*?\*\/)/gm,
                replacement: '<span class="comment">$1</span>'
            },
            // Strings (generic)
            {
                pattern: /(['"`])(?:(?=(\\?))\2.)*?\1/g,
                replacement: '<span class="string">$&</span>'
            },
            // Numbers
            {
                pattern: /\b(\d+\.?\d*)\b/g,
                replacement: '<span class="number">$1</span>'
            }
        ];
    }
}

// Create highlighter instance
const highlighter = new WorkerHighlighter();

// Handle messages from main thread
self.addEventListener('message', (event) => {
    const { id, text, language } = event.data;
    
    try {
        // Perform highlighting
        const highlighted = highlighter.highlight(text, language);
        
        // Send result back
        self.postMessage({
            id,
            highlighted,
            success: true
        });
    } catch (error) {
        // Send error back
        self.postMessage({
            id,
            error: error.message,
            success: false
        });
    }
});

// Notify that worker is ready
self.postMessage({ ready: true });