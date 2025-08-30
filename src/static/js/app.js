class LocalPaste {
    constructor() {
        this.currentPaste = null;
        this.editor = null;
        this.pastes = [];
        this.folders = [];
        this.init().catch(err => {
            console.error('Failed to initialize:', err);
            this.setStatus('Failed to initialize app');
        });
    }

    async init() {
        try {
            // Initialize editor first
            await this.initEditor();
            
            // Bind events
            this.bindEvents();
            
            // Load data
            await Promise.all([
                this.loadFolders(),
                this.loadPastes()
            ]);
            
            // Load first paste if available
            if (this.pastes.length > 0) {
                await this.loadPaste(this.pastes[0].id);
            }
            
            this.setStatus('Ready');
        } catch (err) {
            console.error('Initialization error:', err);
            this.setStatus('Error loading data');
        }
    }

    async initEditor() {
        // Wait for CodeMirror to load
        let retries = 0;
        while (!window.CodeMirror && retries < 20) {
            await new Promise(resolve => setTimeout(resolve, 100));
            retries++;
        }
        
        if (!window.CodeMirror) {
            throw new Error('CodeMirror failed to load');
        }
        
        const { EditorView, basicSetup, markdown, oneDark } = window.CodeMirror;
        
        this.editor = new EditorView({
            extensions: [
                basicSetup,
                markdown(),
                oneDark,
                EditorView.updateListener.of(update => {
                    if (update.docChanged) {
                        this.onEditorChange();
                    }
                    this.updateCursorPosition(update.state);
                })
            ],
            parent: document.getElementById('editor-container')
        });
    }

    bindEvents() {
        // New paste button
        const newBtn = document.getElementById('new-paste');
        if (newBtn) {
            newBtn.addEventListener('click', () => this.createNewPaste());
        }
        
        // Save button
        const saveBtn = document.getElementById('quick-save');
        if (saveBtn) {
            saveBtn.addEventListener('click', () => this.savePaste());
        }
        
        // Delete button
        const deleteBtn = document.getElementById('delete-paste');
        if (deleteBtn) {
            deleteBtn.addEventListener('click', () => this.deletePaste());
        }
        
        // Search input
        const searchInput = document.getElementById('search');
        if (searchInput) {
            searchInput.addEventListener('input', e => this.searchPastes(e.target.value));
        }
        
        // Paste name input
        const nameInput = document.getElementById('paste-name');
        if (nameInput) {
            nameInput.addEventListener('change', () => this.savePaste());
        }
        
        // New folder button
        const folderBtn = document.getElementById('new-folder');
        if (folderBtn) {
            folderBtn.addEventListener('click', () => this.createNewFolder());
        }
        
        // Keyboard shortcuts
        document.addEventListener('keydown', (e) => {
            if (e.ctrlKey || e.metaKey) {
                switch(e.key) {
                    case 's':
                        e.preventDefault();
                        this.savePaste();
                        break;
                    case 'n':
                        e.preventDefault();
                        this.createNewPaste();
                        break;
                    case 'k':
                        e.preventDefault();
                        document.getElementById('search')?.focus();
                        break;
                }
            }
        });
    }

    async createNewPaste() {
        try {
            const res = await fetch('/api/paste', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ content: '', language: 'markdown' })
            });
            
            if (!res.ok) {
                throw new Error(`Failed to create paste: ${res.statusText}`);
            }
            
            const paste = await res.json();
            await this.loadPastes();
            await this.loadPaste(paste.id);
            this.setStatus('New paste created');
        } catch (err) {
            console.error('Error creating paste:', err);
            this.setStatus('Failed to create paste');
        }
    }

    async createNewFolder() {
        const name = prompt('Folder name:');
        if (!name) return;
        
        try {
            const res = await fetch('/api/folder', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ name })
            });
            
            if (!res.ok) {
                throw new Error(`Failed to create folder: ${res.statusText}`);
            }
            
            await this.loadFolders();
            this.setStatus('Folder created');
        } catch (err) {
            console.error('Error creating folder:', err);
            this.setStatus('Failed to create folder');
        }
    }

    async loadFolders() {
        try {
            const res = await fetch('/api/folders');
            if (!res.ok) {
                throw new Error(`Failed to load folders: ${res.statusText}`);
            }
            
            this.folders = await res.json();
            this.renderFolders();
        } catch (err) {
            console.error('Error loading folders:', err);
            this.folders = [];
        }
    }
    
    renderFolders() {
        const container = document.getElementById('folder-list');
        if (!container) return;
        
        // Clear and add "All Pastes"
        container.innerHTML = '';
        
        const allItem = document.createElement('li');
        allItem.className = 'active';
        allItem.dataset.folder = 'all';
        allItem.textContent = 'All Pastes';
        allItem.addEventListener('click', () => {
            this.selectFolder(null);
        });
        container.appendChild(allItem);
        
        // Add folders
        this.folders.forEach(folder => {
            const li = document.createElement('li');
            li.dataset.folderId = folder.id;
            li.textContent = `${folder.name} (${folder.paste_count})`;
            li.addEventListener('click', () => this.selectFolder(folder.id));
            container.appendChild(li);
        });
    }

    selectFolder(folderId) {
        // Update active state
        document.querySelectorAll('#folder-list li').forEach(li => {
            li.classList.remove('active');
        });
        
        if (folderId) {
            const li = document.querySelector(`[data-folder-id="${folderId}"]`);
            if (li) li.classList.add('active');
        } else {
            const li = document.querySelector('[data-folder="all"]');
            if (li) li.classList.add('active');
        }
        
        // Load pastes for folder
        this.loadPastes('', folderId);
    }

    async savePaste() {
        if (!this.currentPaste || !this.editor) return;
        
        try {
            const content = this.editor.state.doc.toString();
            const name = document.getElementById('paste-name')?.value || 'untitled';
            
            const res = await fetch(`/api/paste/${this.currentPaste.id}`, {
                method: 'PUT',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ content, name })
            });
            
            if (!res.ok) {
                throw new Error(`Failed to save: ${res.statusText}`);
            }
            
            this.currentPaste = await res.json();
            this.setStatus('Saved');
            await this.loadPastes();
        } catch (err) {
            console.error('Error saving paste:', err);
            this.setStatus('Failed to save');
        }
    }

    async loadPastes(query = '', folderId = null) {
        try {
            let url = '/api/pastes?limit=50';
            
            if (query) {
                url = `/api/search?q=${encodeURIComponent(query)}&limit=50`;
            } else if (folderId) {
                url += `&folder_id=${folderId}`;
            }
            
            const res = await fetch(url);
            if (!res.ok) {
                throw new Error(`Failed to load pastes: ${res.statusText}`);
            }
            
            this.pastes = await res.json();
            this.renderPasteList();
        } catch (err) {
            console.error('Error loading pastes:', err);
            this.pastes = [];
            this.renderPasteList();
        }
    }

    searchPastes(query) {
        clearTimeout(this.searchTimeout);
        this.searchTimeout = setTimeout(() => {
            this.loadPastes(query);
        }, 300);
    }

    renderPasteList() {
        const container = document.getElementById('paste-list');
        if (!container) return;
        
        container.innerHTML = '';
        
        if (this.pastes.length === 0) {
            container.innerHTML = '<li style="opacity: 0.5; font-style: italic;">No pastes yet</li>';
            return;
        }
        
        this.pastes.forEach(paste => {
            const li = document.createElement('li');
            li.dataset.pasteId = paste.id;
            li.className = this.currentPaste?.id === paste.id ? 'active' : '';
            li.textContent = paste.name;
            li.addEventListener('click', () => this.loadPaste(paste.id));
            container.appendChild(li);
        });
    }

    async loadPaste(id) {
        try {
            const res = await fetch(`/api/paste/${id}`);
            if (!res.ok) {
                throw new Error(`Failed to load paste: ${res.statusText}`);
            }
            
            const paste = await res.json();
            this.currentPaste = paste;
            
            // Update editor
            if (this.editor) {
                this.editor.dispatch({
                    changes: { 
                        from: 0, 
                        to: this.editor.state.doc.length, 
                        insert: paste.content 
                    }
                });
            }
            
            // Update UI
            const nameInput = document.getElementById('paste-name');
            if (nameInput) nameInput.value = paste.name;
            
            const langSpan = document.getElementById('paste-language');
            if (langSpan) langSpan.textContent = paste.language || 'plain';
            
            const dateSpan = document.getElementById('paste-date');
            if (dateSpan) {
                const date = new Date(paste.updated_at);
                dateSpan.textContent = date.toLocaleString();
            }
            
            const charSpan = document.getElementById('char-count');
            if (charSpan) {
                charSpan.textContent = `${paste.content.length} chars`;
            }
            
            this.renderPasteList();
        } catch (err) {
            console.error('Error loading paste:', err);
            this.setStatus('Failed to load paste');
        }
    }

    async deletePaste() {
        if (!this.currentPaste) return;
        
        if (!confirm(`Delete "${this.currentPaste.name}"?`)) return;
        
        try {
            const res = await fetch(`/api/paste/${this.currentPaste.id}`, { 
                method: 'DELETE' 
            });
            
            if (!res.ok) {
                throw new Error(`Failed to delete: ${res.statusText}`);
            }
            
            this.currentPaste = null;
            
            if (this.editor) {
                this.editor.dispatch({ 
                    changes: { 
                        from: 0, 
                        to: this.editor.state.doc.length, 
                        insert: '' 
                    } 
                });
            }
            
            await this.loadPastes();
            
            // Load first paste if available
            if (this.pastes.length > 0) {
                await this.loadPaste(this.pastes[0].id);
            }
            
            this.setStatus('Paste deleted');
        } catch (err) {
            console.error('Error deleting paste:', err);
            this.setStatus('Failed to delete');
        }
    }

    onEditorChange() {
        if (!this.currentPaste) return;
        
        const charCount = this.editor.state.doc.length;
        const charSpan = document.getElementById('char-count');
        if (charSpan) {
            charSpan.textContent = `${charCount} chars`;
        }
        
        // Auto-save after 2 seconds
        clearTimeout(this.saveTimeout);
        this.saveTimeout = setTimeout(() => {
            this.savePaste();
        }, 2000);
    }

    updateCursorPosition(state) {
        const pos = state.selection.main.head;
        const line = state.doc.lineAt(pos);
        const col = pos - line.from + 1;
        
        const posSpan = document.getElementById('cursor-position');
        if (posSpan) {
            posSpan.textContent = `Ln ${line.number}, Col ${col}`;
        }
    }

    setStatus(message) {
        const el = document.getElementById('status-message');
        if (el) {
            el.textContent = message;
            clearTimeout(this.statusTimeout);
            if (message !== 'Ready') {
                this.statusTimeout = setTimeout(() => {
                    el.textContent = 'Ready';
                }, 3000);
            }
        }
    }
}

// Initialize app when DOM is ready
if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', () => {
        window.app = new LocalPaste();
    });
} else {
    window.app = new LocalPaste();
}