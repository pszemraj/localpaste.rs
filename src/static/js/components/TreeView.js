/**
 * TreeView Component - Displays folders and pastes in a tree structure
 */

import { formatDate } from '../utils/date.js';

export class TreeViewComponent {
    constructor(store, api, container) {
        this.store = store;
        this.api = api;
        this.container = container;
        this.draggedPaste = null;
        
        // Bind methods
        this.render = this.render.bind(this);
        this.handleFolderClick = this.handleFolderClick.bind(this);
        this.handlePasteClick = this.handlePasteClick.bind(this);
        this.handleDragStart = this.handleDragStart.bind(this);
        this.handleDragOver = this.handleDragOver.bind(this);
        this.handleDrop = this.handleDrop.bind(this);
    }
    
    mount() {
        // Subscribe to store changes
        this.unsubscribe = this.store.subscribe((event) => {
            const relevantActions = [
                'INIT_DATA', 'PASTE_CREATED', 'PASTE_UPDATED', 'PASTE_DELETED',
                'FOLDER_CREATED', 'FOLDER_UPDATED', 'FOLDER_DELETED',
                'FOLDER_TOGGLED', 'PASTE_SELECTED', 'SEARCH_UPDATED'
            ];
            
            if (relevantActions.includes(event.detail.action.type)) {
                this.render();
            }
        });
        
        this.render();
    }
    
    unmount() {
        if (this.unsubscribe) {
            this.unsubscribe();
        }
        this.container.innerHTML = '';
    }
    
    render() {
        const state = this.store.getState();
        const { folders, pastes, ui } = state;
        
        // Filter pastes based on search
        let filteredPastes = Array.from(pastes.values());
        if (ui.searchQuery) {
            const query = ui.searchQuery.toLowerCase();
            filteredPastes = filteredPastes.filter(paste => 
                paste.name.toLowerCase().includes(query) ||
                paste.content.toLowerCase().includes(query)
            );
        }
        
        // Sort pastes
        filteredPastes.sort((a, b) => {
            switch (ui.sortOrder) {
                case 'date-asc':
                    return new Date(a.created_at) - new Date(b.created_at);
                case 'date-desc':
                    return new Date(b.created_at) - new Date(a.created_at);
                case 'name-asc':
                    return a.name.localeCompare(b.name);
                case 'name-desc':
                    return b.name.localeCompare(a.name);
                default:
                    return 0;
            }
        });
        
        // Group pastes by folder
        const pastesByFolder = new Map();
        pastesByFolder.set('unfiled', []);
        
        folders.forEach(folder => {
            pastesByFolder.set(folder.id, []);
        });
        
        filteredPastes.forEach(paste => {
            const folderId = paste.folder_id || 'unfiled';
            if (pastesByFolder.has(folderId)) {
                pastesByFolder.get(folderId).push(paste);
            }
        });
        
        // Build tree HTML
        let html = '';
        
        // Unfiled pastes folder
        const unfiledPastes = pastesByFolder.get('unfiled') || [];
        const unfiledExpanded = ui.expandedFolders.has('unfiled');
        
        html += `
            <li class="tree-item">
                <div class="tree-folder ${unfiledExpanded ? 'expanded' : ''}" 
                     data-folder-id="unfiled">
                    <span class="tree-folder-icon">▶</span>
                    <span>Unfiled</span>
                    <span style="margin-left: auto; opacity: 0.6; font-size: 12px;">
                        ${unfiledPastes.length}
                    </span>
                </div>
                <ul class="tree-contents" ${unfiledExpanded ? 'style="display: block;"' : ''}>
                    ${this.renderPastes(unfiledPastes, ui.currentPasteId)}
                </ul>
            </li>
        `;
        
        // User folders
        folders.forEach(folder => {
            const folderPastes = pastesByFolder.get(folder.id) || [];
            const isExpanded = ui.expandedFolders.has(folder.id);
            
            html += `
                <li class="tree-item">
                    <div class="tree-folder ${isExpanded ? 'expanded' : ''}" 
                         data-folder-id="${folder.id}"
                         draggable="false">
                        <span class="tree-folder-icon">▶</span>
                        <span>${this.escapeHtml(folder.name)}</span>
                        <span style="margin-left: auto; display: flex; gap: 8px; align-items: center;">
                            <span style="opacity: 0.6; font-size: 12px;">
                                ${folderPastes.length}
                            </span>
                            <span class="folder-actions">
                                <button data-action="rename" data-folder-id="${folder.id}" 
                                        title="Rename folder">✏️</button>
                                <button data-action="delete" data-folder-id="${folder.id}" 
                                        title="Delete folder">🗑️</button>
                            </span>
                        </span>
                    </div>
                    <ul class="tree-contents" ${isExpanded ? 'style="display: block;"' : ''}>
                        ${this.renderPastes(folderPastes, ui.currentPasteId)}
                    </ul>
                </li>
            `;
        });
        
        this.container.innerHTML = html;
        
        // Attach event listeners
        this.attachEventListeners();
    }
    
    renderPastes(pastes, currentPasteId) {
        if (pastes.length === 0) {
            return '<li style="padding: 4px 8px; opacity: 0.5; font-size: 13px;">No pastes</li>';
        }
        
        return pastes.map(paste => `
            <li class="tree-paste ${paste.id === currentPasteId ? 'active' : ''}"
                data-paste-id="${paste.id}"
                draggable="true">
                <span>${this.escapeHtml(paste.name)}</span>
                <span class="paste-date">${formatDate(paste.created_at)}</span>
            </li>
        `).join('');
    }
    
    attachEventListeners() {
        // Folder clicks
        this.container.querySelectorAll('.tree-folder').forEach(folder => {
            folder.addEventListener('click', this.handleFolderClick);
            folder.addEventListener('dragover', this.handleDragOver);
            folder.addEventListener('drop', this.handleDrop);
        });
        
        // Paste clicks
        this.container.querySelectorAll('.tree-paste').forEach(paste => {
            paste.addEventListener('click', this.handlePasteClick);
            paste.addEventListener('dragstart', this.handleDragStart);
        });
        
        // Folder actions
        this.container.querySelectorAll('.folder-actions button').forEach(btn => {
            btn.addEventListener('click', (e) => {
                e.stopPropagation();
                const action = btn.dataset.action;
                const folderId = btn.dataset.folderId;
                
                if (action === 'rename') {
                    this.renameFolder(folderId);
                } else if (action === 'delete') {
                    this.deleteFolder(folderId);
                }
            });
        });
    }
    
    handleFolderClick(e) {
        const folderId = e.currentTarget.dataset.folderId;
        this.store.dispatch({
            type: 'FOLDER_TOGGLED',
            payload: folderId
        });
    }
    
    handlePasteClick(e) {
        const pasteId = e.currentTarget.dataset.pasteId;
        this.store.dispatch({
            type: 'PASTE_SELECTED',
            payload: pasteId
        });
    }
    
    handleDragStart(e) {
        this.draggedPaste = e.currentTarget.dataset.pasteId;
        e.currentTarget.classList.add('dragging');
    }
    
    handleDragOver(e) {
        e.preventDefault();
        e.currentTarget.classList.add('drag-over');
    }
    
    async handleDrop(e) {
        e.preventDefault();
        e.currentTarget.classList.remove('drag-over');
        
        if (!this.draggedPaste) return;
        
        const folderId = e.currentTarget.dataset.folderId;
        
        try {
            // Update paste's folder
            await this.api.updatePaste(this.draggedPaste, { folder_id: folderId });
            
            // Update store
            const paste = this.store.getState().pastes.get(this.draggedPaste);
            if (paste) {
                this.store.dispatch({
                    type: 'PASTE_UPDATED',
                    payload: { ...paste, folder_id: folderId }
                });
            }
        } catch (error) {
            console.error('Failed to move paste:', error);
            this.store.dispatch({
                type: 'ERROR',
                payload: 'Failed to move paste'
            });
        }
        
        // Clean up
        document.querySelectorAll('.dragging').forEach(el => {
            el.classList.remove('dragging');
        });
        this.draggedPaste = null;
    }
    
    async renameFolder(folderId) {
        const folder = this.store.getState().folders.find(f => f.id === folderId);
        if (!folder) return;
        
        const newName = prompt('Enter new folder name:', folder.name);
        if (!newName || newName === folder.name) return;
        
        try {
            const updated = await this.api.updateFolder(folderId, { name: newName });
            this.store.dispatch({
                type: 'FOLDER_UPDATED',
                payload: updated
            });
        } catch (error) {
            console.error('Failed to rename folder:', error);
            this.store.dispatch({
                type: 'ERROR',
                payload: 'Failed to rename folder'
            });
        }
    }
    
    async deleteFolder(folderId) {
        if (!confirm('Delete this folder? Pastes will be moved to Unfiled.')) return;
        
        try {
            await this.api.deleteFolder(folderId);
            this.store.dispatch({
                type: 'FOLDER_DELETED',
                payload: folderId
            });
        } catch (error) {
            console.error('Failed to delete folder:', error);
            this.store.dispatch({
                type: 'ERROR',
                payload: 'Failed to delete folder'
            });
        }
    }
    
    escapeHtml(text) {
        const div = document.createElement('div');
        div.textContent = text;
        return div.innerHTML;
    }
}

export default TreeViewComponent;