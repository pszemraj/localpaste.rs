class LocalPaste {
    constructor() {
        this.currentPaste = null;
        this.editor = null;
        this.pastes = [];
        this.init();
    }

    async init() {
        this.initEditor();
        this.bindEvents();
        await this.loadPastes();
        if (this.pastes.length > 0) {
            this.loadPaste(this.pastes[0].id);
        }
    }

    initEditor() {
        const { EditorView, basicSetup, markdown, oneDark } = window.CodeMirror;
        this.editor = new EditorView({
            extensions: [
                basicSetup,
                markdown(),
                oneDark,
                EditorView.updateListener.of(update => {
                    if (update.docChanged) this.onEditorChange();
                    this.updateCursorPosition(update.state);
                })
            ],
            parent: document.getElementById('editor-container')
        });
    }

    bindEvents() {
        document.getElementById('new-paste').addEventListener('click', () => this.createNewPaste());
        document.getElementById('quick-save').addEventListener('click', () => this.savePaste());
        document.getElementById('delete-paste').addEventListener('click', () => this.deletePaste());
        document.getElementById('search').addEventListener('input', e => this.searchPastes(e.target.value));
        document.getElementById('paste-name').addEventListener('change', () => this.savePaste());
        document.getElementById('new-folder').addEventListener('click', () => this.createNewFolder());
    }

    async createNewPaste() {
        const res = await fetch('/api/paste', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ content: '', language: 'markdown' })
        });
        if (res.ok) {
            const paste = await res.json();
            await this.loadPastes();
            this.loadPaste(paste.id);
            this.setStatus('New paste created');
        }
    }

    async createNewFolder() {
        const name = prompt('Folder name:');
        if (!name) return;
        
        const res = await fetch('/api/folder', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ name })
        });
        if (res.ok) {
            await this.loadFolders();
            this.setStatus('Folder created');
        }
    }

    async loadFolders() {
        const res = await fetch('/api/folders');
        if (res.ok) {
            const folders = await res.json();
            const container = document.getElementById('folder-list');
            container.innerHTML = '<li class="active" data-folder="all">All Pastes</li>';
            folders.forEach(f => {
                const li = document.createElement('li');
                li.dataset.folderId = f.id;
                li.textContent = `${f.name} (${f.paste_count})`;
                li.addEventListener('click', () => this.selectFolder(f.id));
                container.appendChild(li);
            });
        }
    }

    selectFolder(folderId) {
        document.querySelectorAll('#folder-list li').forEach(li => li.classList.remove('active'));
        const li = document.querySelector(`[data-folder-id="${folderId}"]`);
        if (li) li.classList.add('active');
        this.loadPastes('', folderId);
    }

    async savePaste() {
        if (!this.currentPaste) return;
        const content = this.editor.state.doc.toString();
        const res = await fetch(`/api/paste/${this.currentPaste.id}`, {
            method: 'PUT',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ content, name: document.getElementById('paste-name').value })
        });
        if (res.ok) {
            this.setStatus('Saved');
            await this.loadPastes();
        }
    }

    async loadPastes(query = '', folderId = null) {
        let url = '/api/pastes?limit=50';
        if (query) {
            url = `/api/search?q=${encodeURIComponent(query)}&limit=50`;
        } else if (folderId) {
            url += `&folder_id=${folderId}`;
        }
        
        const res = await fetch(url);
        if (res.ok) {
            this.pastes = await res.json();
            this.renderPasteList();
        }
    }

    async searchPastes(query) {
        clearTimeout(this.searchTimeout);
        this.searchTimeout = setTimeout(() => this.loadPastes(query), 300);
    }

    renderPasteList() {
        const container = document.getElementById('paste-list');
        container.innerHTML = this.pastes.map(p => `
            <li data-paste-id="${p.id}" class="${this.currentPaste?.id === p.id ? 'active' : ''}">
                ${p.name}
            </li>
        `).join('');
        container.querySelectorAll('li').forEach(li => {
            li.addEventListener('click', () => this.loadPaste(li.dataset.pasteId));
        });
    }

    async loadPaste(id) {
        const res = await fetch(`/api/paste/${id}`);
        if (res.ok) {
            const paste = await res.json();
            this.currentPaste = paste;
            this.editor.dispatch({
                changes: { from: 0, to: this.editor.state.doc.length, insert: paste.content }
            });
            document.getElementById('paste-name').value = paste.name;
            document.getElementById('paste-language').textContent = paste.language || 'plain';
            document.getElementById('paste-date').textContent = new Date(paste.updated_at).toLocaleString();
            this.renderPasteList();
        }
    }

    async deletePaste() {
        if (!this.currentPaste || !confirm(`Delete "${this.currentPaste.name}"?`)) return;
        const res = await fetch(`/api/paste/${this.currentPaste.id}`, { method: 'DELETE' });
        if (res.ok) {
            this.currentPaste = null;
            this.editor.dispatch({ changes: { from: 0, to: this.editor.state.doc.length, insert: '' } });
            await this.loadPastes();
            this.setStatus('Paste deleted');
        }
    }

    onEditorChange() {
        if (!this.currentPaste) return;
        document.getElementById('char-count').textContent = `${this.editor.state.doc.length} chars`;
        clearTimeout(this.saveTimeout);
        this.saveTimeout = setTimeout(() => this.savePaste(), 2000);
    }

    updateCursorPosition(state) {
        const line = state.doc.lineAt(state.selection.main.head);
        document.getElementById('cursor-position').textContent = `Ln ${line.number}, Col ${state.selection.main.head - line.from + 1}`;
    }

    setStatus(message) {
        const el = document.getElementById('status-message');
        el.textContent = message;
        clearTimeout(this.statusTimeout);
        this.statusTimeout = setTimeout(() => { el.textContent = 'Ready'; }, 3000);
    }
}

document.addEventListener('DOMContentLoaded', () => { 
    window.app = new LocalPaste(); 
});