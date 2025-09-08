// Language definitions for syntax highlighting
// This file is imported by the Web Worker

const LANGUAGES = {
    javascript: {
        keywords: ['const', 'let', 'var', 'function', 'return', 'if', 'else', 'for', 'while', 'do', 'switch', 'case', 'break', 'continue', 'try', 'catch', 'finally', 'throw', 'new', 'typeof', 'instanceof', 'in', 'of', 'async', 'await', 'yield', 'import', 'export', 'from', 'default', 'class', 'extends', 'static', 'get', 'set'],
        types: ['Array', 'Object', 'String', 'Number', 'Boolean', 'Promise', 'Map', 'Set', 'WeakMap', 'WeakSet', 'Symbol', 'undefined', 'null'],
        builtins: ['console', 'window', 'document', 'Math', 'Date', 'JSON', 'parseInt', 'parseFloat', 'isNaN', 'isFinite']
    },
    python: {
        keywords: ['def', 'class', 'if', 'elif', 'else', 'for', 'while', 'return', 'import', 'from', 'as', 'try', 'except', 'finally', 'with', 'lambda', 'yield', 'pass', 'break', 'continue', 'and', 'or', 'not', 'is', 'in', 'True', 'False', 'None', 'self', 'async', 'await', 'global', 'nonlocal', 'del', 'assert', 'raise'],
        types: ['int', 'float', 'str', 'list', 'dict', 'tuple', 'set', 'bool', 'bytes', 'bytearray'],
        builtins: ['print', 'input', 'len', 'range', 'enumerate', 'zip', 'map', 'filter', 'sorted', 'reversed', 'sum', 'min', 'max', 'abs', 'round', 'open', 'type', 'isinstance', 'hasattr', 'getattr', 'setattr']
    },
    rust: {
        keywords: ['fn', 'let', 'mut', 'const', 'if', 'else', 'match', 'for', 'while', 'loop', 'return', 'use', 'mod', 'pub', 'struct', 'enum', 'impl', 'trait', 'type', 'where', 'async', 'await', 'move', 'ref', 'break', 'continue', 'self', 'Self', 'super', 'crate', 'static', 'extern', 'unsafe', 'as', 'in', 'dyn'],
        types: ['bool', 'char', 'i8', 'i16', 'i32', 'i64', 'i128', 'isize', 'u8', 'u16', 'u32', 'u64', 'u128', 'usize', 'f32', 'f64', 'str', 'String', 'Vec', 'Option', 'Result', 'Box', 'Rc', 'Arc', 'RefCell', 'Mutex', 'RwLock'],
        macros: ['println!', 'print!', 'eprintln!', 'eprint!', 'format!', 'panic!', 'assert!', 'assert_eq!', 'assert_ne!', 'debug_assert!', 'vec!', 'include!', 'include_str!', 'include_bytes!', 'env!', 'option_env!', 'concat!', 'line!', 'column!', 'file!', 'stringify!']
    },
    go: {
        keywords: ['package', 'import', 'func', 'var', 'const', 'type', 'struct', 'interface', 'if', 'else', 'switch', 'case', 'default', 'for', 'range', 'return', 'break', 'continue', 'goto', 'defer', 'go', 'select', 'chan', 'map', 'nil', 'true', 'false'],
        types: ['bool', 'byte', 'complex64', 'complex128', 'error', 'float32', 'float64', 'int', 'int8', 'int16', 'int32', 'int64', 'rune', 'string', 'uint', 'uint8', 'uint16', 'uint32', 'uint64', 'uintptr'],
        builtins: ['append', 'cap', 'close', 'complex', 'copy', 'delete', 'imag', 'len', 'make', 'new', 'panic', 'print', 'println', 'real', 'recover']
    },
    html: {
        tags: ['html', 'head', 'body', 'title', 'meta', 'link', 'script', 'style', 'div', 'span', 'p', 'a', 'img', 'ul', 'ol', 'li', 'table', 'tr', 'td', 'th', 'form', 'input', 'button', 'select', 'option', 'textarea', 'label', 'header', 'footer', 'nav', 'main', 'section', 'article', 'aside', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6'],
        attributes: ['id', 'class', 'style', 'src', 'href', 'alt', 'title', 'type', 'name', 'value', 'placeholder', 'required', 'disabled', 'readonly', 'checked', 'selected', 'multiple', 'data-', 'aria-']
    },
    css: {
        properties: ['display', 'position', 'top', 'right', 'bottom', 'left', 'width', 'height', 'margin', 'padding', 'border', 'background', 'color', 'font-size', 'font-family', 'font-weight', 'text-align', 'text-decoration', 'line-height', 'flex', 'grid', 'overflow', 'z-index', 'opacity', 'transform', 'transition', 'animation'],
        values: ['block', 'inline', 'inline-block', 'flex', 'grid', 'none', 'absolute', 'relative', 'fixed', 'sticky', 'auto', 'inherit', 'initial', 'unset', 'center', 'left', 'right', 'justify', 'bold', 'normal', 'italic', 'underline', 'solid', 'dashed', 'dotted', 'transparent'],
        units: ['px', 'em', 'rem', '%', 'vh', 'vw', 'vmin', 'vmax', 'ch', 'ex', 'cm', 'mm', 'in', 'pt', 'pc', 'deg', 'rad', 'turn', 's', 'ms']
    },
    json: {
        keywords: ['true', 'false', 'null']
    },
    yaml: {
        keywords: ['true', 'false', 'null', 'yes', 'no', 'on', 'off']
    },
    toml: {
        keywords: ['true', 'false']
    },
    markdown: {
        keywords: []
    }
};

// Export for use in other modules if needed
if (typeof module !== 'undefined' && module.exports) {
    module.exports = LANGUAGES;
}