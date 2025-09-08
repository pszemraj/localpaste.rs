export class Editor {
    constructor(editorElement, options = {}) {
        this.element = editorElement;
        this.highlightLayer = options.highlightLayer;
        this.onInput = options.onInput || (() => {});
        this.onCursorChange = options.onCursorChange || (() => {});
        this.setupEventListeners();
    }

    setupEventListeners() {
        // Input handling
        this.element.addEventListener('input', () => {
            this.onInput(this.getValue());
        });

        // Sync scroll with highlight layer
        if (this.highlightLayer) {
            this.element.addEventListener('scroll', () => {
                this.highlightLayer.scrollTop = this.element.scrollTop;
            });
        }

        // Cursor position tracking
        this.element.addEventListener('keyup', () => {
            this.onCursorChange(this.getCursorPosition());
        });
        
        this.element.addEventListener('click', () => {
            this.onCursorChange(this.getCursorPosition());
        });

        // Tab handling
        this.element.addEventListener('keydown', (e) => {
            if (e.key === 'Tab') {
                e.preventDefault();
                this.insertTab();
            }
        });
    }

    getValue() {
        return this.element.value;
    }

    setValue(value) {
        this.element.value = value;
    }

    clear() {
        this.element.value = '';
    }

    focus() {
        this.element.focus();
    }

    getCursorPosition() {
        const pos = this.element.selectionStart;
        const text = this.element.value.substring(0, pos);
        const lines = text.split('\n');
        const line = lines.length;
        const col = lines[lines.length - 1].length + 1;
        return { line, col, position: pos };
    }

    setCursorPosition(position) {
        this.element.selectionStart = position;
        this.element.selectionEnd = position;
    }

    getSelection() {
        const start = this.element.selectionStart;
        const end = this.element.selectionEnd;
        return {
            text: this.element.value.substring(start, end),
            start,
            end
        };
    }

    replaceSelection(text) {
        const start = this.element.selectionStart;
        const end = this.element.selectionEnd;
        const value = this.element.value;
        
        this.element.value = value.substring(0, start) + text + value.substring(end);
        this.element.selectionStart = start + text.length;
        this.element.selectionEnd = start + text.length;
        
        // Trigger input event
        this.element.dispatchEvent(new Event('input'));
    }

    insertTab() {
        const start = this.element.selectionStart;
        const end = this.element.selectionEnd;
        const value = this.element.value;
        
        if (start === end) {
            // No selection, insert tab
            this.replaceSelection('    ');
        } else {
            // Multi-line selection, indent each line
            const selectedText = value.substring(start, end);
            const lines = selectedText.split('\n');
            const indentedLines = lines.map(line => '    ' + line);
            this.replaceSelection(indentedLines.join('\n'));
        }
    }

    insertTextAtCursor(text) {
        const start = this.element.selectionStart;
        const value = this.element.value;
        
        this.element.value = value.substring(0, start) + text + value.substring(start);
        this.element.selectionStart = start + text.length;
        this.element.selectionEnd = start + text.length;
        
        // Trigger input event
        this.element.dispatchEvent(new Event('input'));
    }

    getCharCount() {
        return this.element.value.length;
    }

    getLineCount() {
        return this.element.value.split('\n').length;
    }

    getWordCount() {
        const text = this.element.value.trim();
        if (!text) return 0;
        return text.split(/\s+/).length;
    }

    scrollToTop() {
        this.element.scrollTop = 0;
        if (this.highlightLayer) {
            this.highlightLayer.scrollTop = 0;
        }
    }

    scrollToBottom() {
        this.element.scrollTop = this.element.scrollHeight;
        if (this.highlightLayer) {
            this.highlightLayer.scrollTop = this.element.scrollHeight;
        }
    }

    scrollToCursor() {
        // Get cursor position in pixels (approximate)
        const lines = this.element.value.substring(0, this.element.selectionStart).split('\n');
        const lineHeight = parseInt(window.getComputedStyle(this.element).lineHeight);
        const cursorTop = (lines.length - 1) * lineHeight;
        
        // Scroll to make cursor visible
        const editorHeight = this.element.clientHeight;
        const scrollTop = this.element.scrollTop;
        
        if (cursorTop < scrollTop) {
            this.element.scrollTop = cursorTop;
        } else if (cursorTop > scrollTop + editorHeight - lineHeight) {
            this.element.scrollTop = cursorTop - editorHeight + lineHeight * 2;
        }
    }

    // Utility method to handle undo/redo stack (optional)
    createSnapshot() {
        return {
            value: this.element.value,
            selectionStart: this.element.selectionStart,
            selectionEnd: this.element.selectionEnd
        };
    }

    restoreSnapshot(snapshot) {
        this.element.value = snapshot.value;
        this.element.selectionStart = snapshot.selectionStart;
        this.element.selectionEnd = snapshot.selectionEnd;
    }
}

// Export for non-module environments
if (typeof window !== 'undefined') {
    window.ModularEditor = Editor;
}