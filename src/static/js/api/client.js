/**
 * API Client for LocalPaste
 */
export class ApiClient {
    constructor(baseUrl = '') {
        this.baseUrl = baseUrl;
    }

    /**
     * Make an API request
     * @private
     */
    async request(method, endpoint, body = null) {
        const options = {
            method,
            headers: {
                'Content-Type': 'application/json',
            },
        };

        if (body) {
            options.body = JSON.stringify(body);
        }

        const response = await fetch(`${this.baseUrl}${endpoint}`, options);
        
        if (!response.ok) {
            const error = new Error(`API Error: ${response.statusText}`);
            error.status = response.status;
            throw error;
        }

        const contentType = response.headers.get('content-type');
        if (contentType && contentType.includes('application/json')) {
            return response.json();
        }
        
        return response.text();
    }

    // Paste operations
    async createPaste(paste) {
        return this.request('POST', '/api/pastes', paste);
    }

    async getPaste(id) {
        return this.request('GET', `/api/pastes/${id}`);
    }

    async updatePaste(id, updates) {
        return this.request('PUT', `/api/pastes/${id}`, updates);
    }

    async deletePaste(id) {
        return this.request('DELETE', `/api/pastes/${id}`);
    }

    async listPastes(limit = 100, folderId = null) {
        const params = new URLSearchParams({ limit });
        if (folderId) {
            params.append('folder_id', folderId);
        }
        return this.request('GET', `/api/pastes?${params}`);
    }

    async searchPastes(query, limit = 20, folderId = null) {
        const params = new URLSearchParams({ q: query, limit });
        if (folderId) {
            params.append('folder_id', folderId);
        }
        return this.request('GET', `/api/pastes/search?${params}`);
    }

    async duplicatePaste(id) {
        return this.request('POST', `/api/pastes/${id}/duplicate`);
    }

    async exportPaste(id, format) {
        return this.request('GET', `/api/pastes/${id}/export?format=${format}`);
    }

    // Folder operations
    async createFolder(folder) {
        return this.request('POST', '/api/folders', folder);
    }

    async listFolders() {
        return this.request('GET', '/api/folders');
    }

    async updateFolder(id, updates) {
        return this.request('PUT', `/api/folders/${id}`, updates);
    }

    async deleteFolder(id) {
        return this.request('DELETE', `/api/folders/${id}`);
    }
}