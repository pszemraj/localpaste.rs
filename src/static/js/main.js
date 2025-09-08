/**
 * Main entry point for LocalPaste
 */
import { ApiClient } from './api/client.js';
import { SyntaxHighlighter } from './syntax/highlighter.js';
import { debounce } from './utils/debounce.js';
import { ErrorBoundary } from './utils/errors.js';
import { escapeHtml, $, $$ } from './utils/dom.js';

class LocalPaste {
    constructor() {
        this.api = new ApiClient();
        this.highlighter = new SyntaxHighlighter();
        this.errorBoundary = new ErrorBoundary(document.body);
        
        this.currentPaste = null;
        this.pastes = [];
        this.folders = [];
        this.editor = null;
        this.draggedPaste = null;
        this.expandedFolders = new Set(['unfiled']);
        this.sortOrder = 'date-desc';
        
        // Create debounced save function
        this.debouncedSave = debounce(this.savePaste.bind(this), 1000);
        
        console.log('LocalPaste.rs: Initializing...');
    }

    async init() {
        try {
            // Wait for DOM to be ready
            if (document.readyState === 'loading') {
                await new Promise(resolve => {
                    document.addEventListener('DOMContentLoaded', resolve);
                });
            }
            
            this.editor = $('#editor');
            if (!this.editor) {
                throw new Error('Editor element not found');
            }
            
            this.bindEvents();
            await this.loadFolders();
            await this.loadPastes();
            console.log('LocalPaste.rs: Ready');
        } catch (err) {
            console.error('Failed to initialize:', err);
            this.setStatus('Failed to initialize');
        }
    }

    bindEvents() {
        // Editor events
        this.editor.addEventListener('input', () => {
            this.onEditorChange();
        });
        
        // Sync scroll between editor and highlight layer
        this.editor.addEventListener('scroll', () => {
            const highlightLayer = $('#highlight-layer');
            if (highlightLayer) {
                highlightLayer.scrollTop = this.editor.scrollTop;
                highlightLayer.scrollLeft = this.editor.scrollLeft;
            }
        });
        
        // Update cursor position
        this.editor.addEventListener('click', () => this.updateCursorPosition());
        this.editor.addEventListener('keyup', () => this.updateCursorPosition());
        
        // New paste button
        const newBtn = $('#new-paste-btn');
        if (newBtn) {
            newBtn.addEventListener('click', () => this.createNewPaste());
        }
        
        // Search
        const searchInput = $('#search-input');
        if (searchInput) {
            searchInput.addEventListener('input', debounce(() => this.performSearch(), 300));
        }
        
        // Clear search
        const clearSearchBtn = $('#clear-search');
        if (clearSearchBtn) {
            clearSearchBtn.addEventListener('click', () => {
                searchInput.value = '';
                this.performSearch();
            });
        }
        
        // Language selector
        const langSelect = $('#paste-language');
        if (langSelect) {
            langSelect.addEventListener('change', () => {
                if (this.currentPaste) {
                    this.currentPaste.language = langSelect.value || null;
                    this.savePaste();
                    this.updateHighlighting();
                }
            });
        }
        
        // Sort order
        const sortSelect = $('#sort-order');
        if (sortSelect) {
            sortSelect.addEventListener('change', () => {
                this.sortOrder = sortSelect.value;
                this.renderPasteList();
            });
        }
        
        // Duplicate button
        const duplicateBtn = $('#duplicate-btn');
        if (duplicateBtn) {
            duplicateBtn.addEventListener('click', () => this.duplicatePaste());
        }
        
        // Export button
        const exportBtn = $('#export-btn');
        if (exportBtn) {
            exportBtn.addEventListener('click', () => this.showExportMenu());
        }
        
        // Download button
        const downloadBtn = $('#download-btn');
        if (downloadBtn) {
            downloadBtn.addEventListener('click', () => this.downloadPaste());
        }
        
        // Delete button
        const deleteBtn = $('#delete-btn');
        if (deleteBtn) {
            deleteBtn.addEventListener('click', () => this.deletePaste());
        }
        
        // Keyboard shortcuts
        document.addEventListener('keydown', (e) => this.handleKeyboardShortcut(e));
        
        // Prevent navigation away with unsaved changes
        window.addEventListener('beforeunload', (e) => {
            if (this.hasUnsavedChanges()) {
                e.preventDefault();
                e.returnValue = '';
            }
        });
    }

    onEditorChange() {
        if (!this.currentPaste) return;
        
        const charCount = this.editor.value.length;
        const charCountEl = $('#char-count');
        if (charCountEl) {
            charCountEl.textContent = `${charCount} chars`;
        }
        
        // Update highlighting
        this.updateHighlighting();
        
        // Auto-save with proper debouncing
        this.debouncedSave();
    }

    updateHighlighting() {
        const highlightLayer = $('#highlight-layer');
        if (!highlightLayer || !this.currentPaste) return;
        
        const language = this.currentPaste.language || 'plaintext';
        const highlighted = this.highlighter.highlight(this.editor.value, language);
        highlightLayer.innerHTML = highlighted;
    }

    async savePaste() {
        if (!this.currentPaste) return;
        
        try {
            const updates = {
                content: this.editor.value,
                language: this.currentPaste.language,
                folder_id: this.currentPaste.folder_id
            };
            
            const updated = await this.api.updatePaste(this.currentPaste.id, updates);
            
            // Update local state
            this.currentPaste = updated;
            const index = this.pastes.findIndex(p => p.id === updated.id);
            if (index !== -1) {
                this.pastes[index] = updated;
            }
            
            this.setStatus('Saved');
            
            // Update the paste item in the list
            const pasteEl = $(`.paste-item[data-id="${updated.id}"]`);
            if (pasteEl) {
                const previewEl = pasteEl.querySelector('.paste-preview');
                if (previewEl) {
                    previewEl.textContent = this.getPreview(updated.content);
                }
            }
        } catch (err) {
            console.error('Failed to save:', err);
            this.setStatus('Failed to save');
        }
    }

    async loadPastes() {
        try {
            this.pastes = await this.api.listPastes(100);
            this.renderPasteList();
            
            // Load first paste or create new if none exist
            if (this.pastes.length > 0) {
                await this.loadPaste(this.pastes[0].id);
            } else {
                await this.createNewPaste();
            }
        } catch (err) {
            console.error('Failed to load pastes:', err);
            this.setStatus('Failed to load pastes');
        }
    }

    async loadFolders() {
        try {
            this.folders = await this.api.listFolders();
            this.renderFolderList();
        } catch (err) {
            console.error('Failed to load folders:', err);
            this.setStatus('Failed to load folders');
        }
    }

    async loadPaste(id) {
        try {
            const paste = await this.api.getPaste(id);
            this.currentPaste = paste;
            this.editor.value = paste.content || '';
            
            // Update language selector
            const langSelect = $('#paste-language');
            if (langSelect) {
                langSelect.value = paste.language || '';
            }
            
            // Update highlighting
            this.updateHighlighting();
            
            // Update UI elements
            this.updatePasteIdDisplay();
            this.updateCursorPosition();
            this.onEditorChange();
            
            // Update active state in list
            $$('.paste-item').forEach(el => el.classList.remove('active'));
            const pasteEl = $(`.paste-item[data-id="${id}"]`);
            if (pasteEl) {
                pasteEl.classList.add('active');
            }
            
            this.setStatus('Loaded');
        } catch (err) {
            console.error('Failed to load paste:', err);
            this.setStatus('Failed to load paste');
        }
    }

    async createNewPaste() {
        try {
            const paste = {
                name: this.generatePasteName(),
                content: '',
                language: null,
                folder_id: null
            };
            
            const created = await this.api.createPaste(paste);
            this.pastes.unshift(created);
            this.renderPasteList();
            await this.loadPaste(created.id);
            
            this.setStatus('New paste created');
        } catch (err) {
            console.error('Failed to create paste:', err);
            this.setStatus('Failed to create paste');
        }
    }

    async deletePaste() {
        if (!this.currentPaste) return;
        
        if (!confirm('Delete this paste?')) return;
        
        try {
            await this.api.deletePaste(this.currentPaste.id);
            
            // Remove from local state
            this.pastes = this.pastes.filter(p => p.id !== this.currentPaste.id);
            
            // Load another paste or create new
            if (this.pastes.length > 0) {
                await this.loadPaste(this.pastes[0].id);
            } else {
                await this.createNewPaste();
            }
            
            this.renderPasteList();
            this.setStatus('Paste deleted');
        } catch (err) {
            console.error('Failed to delete:', err);
            this.setStatus('Failed to delete');
        }
    }

    async duplicatePaste() {
        if (!this.currentPaste) return;
        
        try {
            const duplicated = await this.api.duplicatePaste(this.currentPaste.id);
            this.pastes.unshift(duplicated);
            this.renderPasteList();
            await this.loadPaste(duplicated.id);
            
            this.setStatus('Paste duplicated');
        } catch (err) {
            console.error('Failed to duplicate:', err);
            this.setStatus('Failed to duplicate');
        }
    }

    async performSearch() {
        const query = $('#search-input').value.trim();
        
        if (!query) {
            await this.loadPastes();
            return;
        }
        
        try {
            this.pastes = await this.api.searchPastes(query, 20);
            this.renderPasteList();
            
            if (this.pastes.length > 0 && !this.pastes.find(p => p.id === this.currentPaste?.id)) {
                await this.loadPaste(this.pastes[0].id);
            }
        } catch (err) {
            console.error('Search failed:', err);
            this.setStatus('Search failed');
        }
    }

    renderPasteList() {
        const container = $('#paste-list');
        if (!container) return;
        
        // Sort pastes
        const sorted = this.sortPastes([...this.pastes]);
        
        // Group by folder
        const grouped = this.groupPastesByFolder(sorted);
        
        // Render tree view
        container.innerHTML = this.renderTreeView(grouped);
        
        // Attach event listeners
        this.attachTreeEventListeners();
    }

    renderFolderList() {
        // Update folder selectors
        const folderSelects = $$('.folder-select');
        folderSelects.forEach(select => {
            const currentValue = select.value;
            select.innerHTML = '<option value="">Unfiled</option>';
            
            this.folders.forEach(folder => {
                const option = document.createElement('option');
                option.value = folder.id;
                option.textContent = folder.name;
                select.appendChild(option);
            });
            
            select.value = currentValue;
        });
    }

    sortPastes(pastes) {
        switch (this.sortOrder) {
            case 'date-desc':
                return pastes.sort((a, b) => new Date(b.updated_at) - new Date(a.updated_at));
            case 'date-asc':
                return pastes.sort((a, b) => new Date(a.updated_at) - new Date(b.updated_at));
            case 'name-asc':
                return pastes.sort((a, b) => a.name.localeCompare(b.name));
            case 'name-desc':
                return pastes.sort((a, b) => b.name.localeCompare(a.name));
            default:
                return pastes;
        }
    }

    groupPastesByFolder(pastes) {
        const grouped = new Map();
        
        // Initialize with all folders
        this.folders.forEach(folder => {
            grouped.set(folder.id, { folder, pastes: [] });
        });
        
        // Add unfiled category
        grouped.set('unfiled', { folder: null, pastes: [] });
        
        // Group pastes
        pastes.forEach(paste => {
            const folderId = paste.folder_id || 'unfiled';
            if (grouped.has(folderId)) {
                grouped.get(folderId).pastes.push(paste);
            } else {
                grouped.get('unfiled').pastes.push(paste);
            }
        });
        
        return grouped;
    }

    renderTreeView(grouped) {
        let html = '';
        
        grouped.forEach((data, folderId) => {
            const { folder, pastes } = data;
            const isExpanded = this.expandedFolders.has(folderId);
            const hasItems = pastes.length > 0;
            
            html += `
                <div class="folder-item" data-folder-id="${folderId}">
                    <div class="folder-header ${hasItems ? 'has-items' : ''}" data-folder-id="${folderId}">
                        <span class="folder-arrow ${isExpanded ? 'expanded' : ''}">${hasItems ? '▶' : ''}</span>
                        <span class="folder-name">${folder ? escapeHtml(folder.name) : 'Unfiled'}</span>
                        <span class="folder-count">${pastes.length}</span>
                    </div>
                    <div class="folder-content ${isExpanded ? 'expanded' : ''}">
                        ${pastes.map(paste => this.renderPasteItem(paste)).join('')}
                    </div>
                </div>
            `;
        });
        
        return html;
    }

    renderPasteItem(paste) {
        const isActive = this.currentPaste?.id === paste.id;
        const preview = this.getPreview(paste.content);
        const date = new Date(paste.updated_at).toLocaleDateString();
        
        return `
            <div class="paste-item ${isActive ? 'active' : ''}" 
                 data-id="${paste.id}"
                 data-folder-id="${paste.folder_id || 'unfiled'}"
                 draggable="true">
                <div class="paste-name">${escapeHtml(paste.name)}</div>
                <div class="paste-preview">${escapeHtml(preview)}</div>
                <div class="paste-date">${date}</div>
            </div>
        `;
    }

    attachTreeEventListeners() {
        // Folder expand/collapse
        $$('.folder-header').forEach(header => {
            header.addEventListener('click', (e) => {
                const folderId = header.dataset.folderId;
                const arrow = header.querySelector('.folder-arrow');
                const content = header.nextElementSibling;
                
                if (this.expandedFolders.has(folderId)) {
                    this.expandedFolders.delete(folderId);
                    arrow.classList.remove('expanded');
                    content.classList.remove('expanded');
                } else {
                    this.expandedFolders.add(folderId);
                    arrow.classList.add('expanded');
                    content.classList.add('expanded');
                }
            });
        });
        
        // Paste item click
        $$('.paste-item').forEach(item => {
            item.addEventListener('click', () => {
                this.loadPaste(item.dataset.id);
            });
            
            // Drag and drop
            item.addEventListener('dragstart', (e) => {
                this.draggedPaste = item.dataset.id;
                e.dataTransfer.effectAllowed = 'move';
                item.classList.add('dragging');
            });
            
            item.addEventListener('dragend', () => {
                item.classList.remove('dragging');
                this.draggedPaste = null;
            });
        });
        
        // Folder drop zones
        $$('.folder-header').forEach(header => {
            header.addEventListener('dragover', (e) => {
                e.preventDefault();
                e.dataTransfer.dropEffect = 'move';
                header.classList.add('drag-over');
            });
            
            header.addEventListener('dragleave', () => {
                header.classList.remove('drag-over');
            });
            
            header.addEventListener('drop', async (e) => {
                e.preventDefault();
                header.classList.remove('drag-over');
                
                if (!this.draggedPaste) return;
                
                const targetFolderId = header.dataset.folderId === 'unfiled' ? null : header.dataset.folderId;
                await this.movePasteToFolder(this.draggedPaste, targetFolderId);
            });
        });
    }

    async movePasteToFolder(pasteId, folderId) {
        try {
            const paste = this.pastes.find(p => p.id === pasteId);
            if (!paste) return;
            
            const updates = {
                folder_id: folderId || ''
            };
            
            const updated = await this.api.updatePaste(pasteId, updates);
            
            // Update local state
            const index = this.pastes.findIndex(p => p.id === pasteId);
            if (index !== -1) {
                this.pastes[index] = updated;
            }
            
            if (this.currentPaste?.id === pasteId) {
                this.currentPaste = updated;
            }
            
            this.renderPasteList();
            await this.loadFolders(); // Refresh folder counts
            
            this.setStatus('Moved to folder');
        } catch (err) {
            console.error('Failed to move paste:', err);
            this.setStatus('Failed to move paste');
        }
    }

    handleKeyboardShortcut(e) {
        // Ctrl/Cmd+S: Save
        if ((e.ctrlKey || e.metaKey) && e.key === 's') {
            e.preventDefault();
            this.savePaste();
        }
        
        // Ctrl/Cmd+N: New paste
        if ((e.ctrlKey || e.metaKey) && e.key === 'n') {
            e.preventDefault();
            this.createNewPaste();
        }
        
        // Ctrl/Cmd+D: Duplicate
        if ((e.ctrlKey || e.metaKey) && e.key === 'd') {
            e.preventDefault();
            this.duplicatePaste();
        }
        
        // Ctrl/Cmd+F: Focus search
        if ((e.ctrlKey || e.metaKey) && e.key === 'f') {
            e.preventDefault();
            const searchInput = $('#search-input');
            if (searchInput) {
                searchInput.focus();
                searchInput.select();
            }
        }
    }

    updateCursorPosition() {
        const pos = this.editor.selectionStart;
        const text = this.editor.value.substring(0, pos);
        const lines = text.split('\n');
        const line = lines.length;
        const col = lines[lines.length - 1].length + 1;
        
        const cursorEl = $('#cursor-position');
        if (cursorEl) {
            cursorEl.textContent = `Ln ${line}, Col ${col}`;
        }
    }

    updatePasteIdDisplay() {
        const pasteIdEl = $('#paste-id');
        if (!pasteIdEl) return;
        
        if (this.currentPaste?.id) {
            pasteIdEl.textContent = `ID: ${this.currentPaste.id}`;
            pasteIdEl.style.display = 'inline';
            
            // Add click to copy
            pasteIdEl.onclick = () => {
                navigator.clipboard.writeText(this.currentPaste.id).then(() => {
                    const originalText = pasteIdEl.textContent;
                    pasteIdEl.textContent = 'Copied!';
                    setTimeout(() => {
                        pasteIdEl.textContent = originalText;
                    }, 1000);
                });
            };
        } else {
            pasteIdEl.style.display = 'none';
        }
    }

    showExportMenu() {
        // Implementation would show export format options
        console.log('Export menu - to be implemented');
    }

    downloadPaste() {
        if (!this.currentPaste) return;
        
        const blob = new Blob([this.currentPaste.content], { type: 'text/plain' });
        const url = URL.createObjectURL(blob);
        const a = document.createElement('a');
        a.href = url;
        a.download = `${this.currentPaste.name}.txt`;
        a.click();
        URL.revokeObjectURL(url);
        
        this.setStatus('Downloaded');
    }

    detectLanguage() {
        // Simple language detection based on content
        // This would be more sophisticated in production
        const content = this.editor.value;
        const langSelect = $('#paste-language');
        
        if (!langSelect || langSelect.value) return;
        
        // Simple heuristics
        if (content.includes('def ') || content.includes('import ')) {
            langSelect.value = 'python';
        } else if (content.includes('function ') || content.includes('const ')) {
            langSelect.value = 'javascript';
        } else if (content.includes('fn ') || content.includes('let mut')) {
            langSelect.value = 'rust';
        }
        // Add more detection logic as needed
    }

    generatePasteName() {
        const adjectives = ['Quick', 'Clever', 'Bright', 'Swift', 'Smart'];
        const nouns = ['Note', 'Snippet', 'Code', 'Text', 'Draft'];
        const adj = adjectives[Math.floor(Math.random() * adjectives.length)];
        const noun = nouns[Math.floor(Math.random() * nouns.length)];
        const num = Math.floor(Math.random() * 1000);
        return `${adj} ${noun} ${num}`;
    }

    getPreview(content) {
        const firstLine = content.split('\n')[0] || '';
        return firstLine.length > 50 ? firstLine.substring(0, 50) + '...' : firstLine;
    }

    hasUnsavedChanges() {
        if (!this.currentPaste || !this.editor) return false;
        return this.editor.value !== (this.currentPaste.content || '');
    }

    setStatus(message) {
        const statusEl = $('#status-message');
        if (statusEl) {
            statusEl.textContent = message;
            setTimeout(() => {
                if (statusEl.textContent === message) {
                    statusEl.textContent = '';
                }
            }, 3000);
        }
    }
}

// Initialize app when DOM is ready
const app = new LocalPaste();
app.init().catch(err => {
    console.error('Failed to start LocalPaste:', err);
});

// Export for debugging
window.LocalPaste = app;