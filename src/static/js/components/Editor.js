/**
 * Editor Component
 * Manages the code editor, syntax highlighting, and auto-save
 */

export class Editor {
    constructor(store, api) {
        this.store = store;
        this.api = api;
        this.element = null;
        this.textarea = null;
        this.highlightLayer = null;
        this.cleanup = null;
        
        // Use debounce from utils if available
        if (window.CommonUtils && window.CommonUtils.debounce) {
            this.saveDebounced = window.CommonUtils.debounce(this.save.bind(this), 1000);
        } else {
            // Fallback debounce
            let timeout;
            this.saveDebounced = (...args) => {
                clearTimeout(timeout);
                timeout = setTimeout(() => this.save.apply(this, args), 1000);
            };
        }
    }

    mount(container) {
        this.element = container;
        this.render();
        this.attachListeners();
        this.subscribeToStore();
    }

    render() {
        const paste = this.store ? this.store.getCurrentPaste() : null;
        if (!paste && this.store) return;

        // Use template literals for cleaner HTML
        this.element.innerHTML = `
            <div class="editor-wrapper">
                <div class="editor-header">
                    <input type="text" id="paste-name" placeholder="Untitled" 
                           value="${this.escapeHtml(paste?.name || '')}" />
                    <select id="paste-language">
                        <option value="">Plain Text</option>
                        <option value="javascript">JavaScript</option>
                        <option value="python">Python</option>
                        <option value="rust">Rust</option>
                        <option value="go">Go</option>
                        <option value="html">HTML</option>
                        <option value="css">CSS</option>
                        <option value="json">JSON</option>
                        <option value="yaml">YAML</option>
                        <option value="toml">TOML</option>
                        <option value="markdown">Markdown</option>
                    </select>
                    <span id="paste-id" class="paste-id">${paste?.id?.slice(0, 8) || ''}</span>
                </div>
                <div class="editor-container">
                    <div id="highlight-layer" class="highlight-layer"></div>
                    <textarea id="editor" class="editor-textarea"
                              placeholder="Start typing or paste your code here..."
                              spellcheck="false">${this.escapeHtml(paste?.content || '')}</textarea>
                </div>
                <div class="editor-footer">
                    <span id="status" class="status-text">Ready</span>
                    <span id="char-count" class="char-count">0 chars</span>
                </div>
            </div>
        `;

        // Cache element references
        this.textarea = this.element.querySelector('#editor');
        this.highlightLayer = this.element.querySelector('#highlight-layer');
        
        // Set language if available
        const langSelect = this.element.querySelector('#paste-language');
        if (langSelect && paste?.language) {
            langSelect.value = paste.language;
        }

        // Update initial counts
        this.updateCharCount();
        this.updateHighlighting();
    }

    attachListeners() {
        if (!this.textarea) return;

        // Editor input
        this.textarea.addEventListener('input', this.handleInput.bind(this));
        this.textarea.addEventListener('scroll', this.handleScroll.bind(this));

        // Name change
        const nameInput = this.element.querySelector('#paste-name');
        if (nameInput) {
            nameInput.addEventListener('input', this.handleNameChange.bind(this));
        }

        // Language change
        const langSelect = this.element.querySelector('#paste-language');
        if (langSelect) {
            langSelect.addEventListener('change', this.handleLanguageChange.bind(this));
        }

        // Store cleanup function
        this.cleanup = () => {
            this.textarea.removeEventListener('input', this.handleInput);
            this.textarea.removeEventListener('scroll', this.handleScroll);
            if (nameInput) nameInput.removeEventListener('input', this.handleNameChange);
            if (langSelect) langSelect.removeEventListener('change', this.handleLanguageChange);
        };
    }

    subscribeToStore() {
        if (!this.store) return;

        // Listen for paste changes
        this.storeUnsubscribe = this.store.subscribe((event) => {
            const { action } = event.detail;
            
            if (action.type === 'SET_CURRENT_PASTE' || action.type === 'PASTE_UPDATED') {
                const paste = this.store.getCurrentPaste();
                if (paste && this.textarea) {
                    // Only update if content actually changed
                    if (this.textarea.value !== paste.content) {
                        this.textarea.value = paste.content;
                        this.updateHighlighting();
                        this.updateCharCount();
                    }
                }
            }
        });
    }

    handleInput(e) {
        this.saveDebounced();
        this.updateHighlighting();
        this.updateCharCount();
        
        // Update store
        if (this.store && window.StoreActions) {
            this.store.dispatch(window.StoreActions.setEditorContent(e.target.value));
        }
    }

    handleScroll(e) {
        // Sync scroll position with highlight layer
        if (this.highlightLayer) {
            this.highlightLayer.scrollTop = e.target.scrollTop;
            this.highlightLayer.scrollLeft = e.target.scrollLeft;
        }
    }

    handleNameChange(e) {
        this.saveDebounced();
    }

    handleLanguageChange(e) {
        this.saveDebounced();
        this.updateHighlighting();
    }

    async save() {
        const paste = this.store ? this.store.getCurrentPaste() : null;
        if (!paste || !this.api) return;

        try {
            const nameInput = this.element.querySelector('#paste-name');
            const langSelect = this.element.querySelector('#paste-language');
            
            const updated = await this.api.updatePaste(paste.id, {
                name: nameInput?.value || paste.name,
                content: this.textarea.value,
                language: langSelect?.value || paste.language
            });

            if (this.store && window.StoreActions) {
                this.store.dispatch(window.StoreActions.updatePaste(updated));
            }

            this.setStatus('Saved');
        } catch (error) {
            console.error('Save failed:', error);
            this.setStatus('Save failed');
            
            if (this.store && window.StoreActions) {
                this.store.dispatch(window.StoreActions.setError(error.message));
            }
        }
    }

    updateHighlighting() {
        if (!this.highlightLayer || !this.textarea) return;

        const lang = this.element.querySelector('#paste-language')?.value || '';
        const text = this.textarea.value;

        // Use the modular highlighter if available
        if (window.ModularSyntaxHighlighter) {
            const highlighter = new window.ModularSyntaxHighlighter();
            const highlighted = highlighter.highlight(text, lang);
            this.highlightLayer.innerHTML = highlighted;
        } else {
            // Fallback: no highlighting
            this.highlightLayer.innerHTML = this.escapeHtml(text);
        }
    }

    updateCharCount() {
        const charCount = this.element.querySelector('#char-count');
        if (charCount && this.textarea) {
            const count = this.textarea.value.length;
            charCount.textContent = `${count} chars`;
        }
    }

    setStatus(message) {
        const statusEl = this.element.querySelector('#status');
        if (statusEl) {
            statusEl.textContent = message;
            // Reset after 2 seconds
            setTimeout(() => {
                statusEl.textContent = 'Ready';
            }, 2000);
        }
    }

    escapeHtml(text) {
        if (window.CommonUtils && window.CommonUtils.escapeHtml) {
            return window.CommonUtils.escapeHtml(text);
        }
        const div = document.createElement('div');
        div.textContent = text;
        return div.innerHTML;
    }

    unmount() {
        if (this.cleanup) {
            this.cleanup();
            this.cleanup = null;
        }
        
        if (this.storeUnsubscribe) {
            this.storeUnsubscribe();
            this.storeUnsubscribe = null;
        }
        
        if (this.element) {
            this.element.innerHTML = '';
        }
        
        this.textarea = null;
        this.highlightLayer = null;
    }
}