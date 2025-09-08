import { languages } from './language-patterns.js';

// Use the escapeHtml from dom-helpers if available, otherwise define it
const escapeHtml = (text) => {
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
};

/**
 * Syntax highlighter for code
 */
export class SyntaxHighlighter {
    constructor() {
        this.languages = languages;
    }
    
    highlight(text, language) {
        // Skip highlighting for large files to prevent freezing
        if (text.length > 100000) {  // 100KB limit
            console.log('File too large for syntax highlighting');
            return escapeHtml(text);
        }
        
        if (!language || language === 'plaintext' || !this.languages[language]) {
            return escapeHtml(text);
        }
        
        // Special handling for markdown to process code blocks
        if (language === 'markdown') {
            return this.highlightMarkdown(text);
        }
        
        const patterns = this.languages[language].patterns;
        const tokens = this.tokenize(text, patterns);
        return this.renderTokens(text, tokens);
    }
    
    highlightMarkdown(text) {
        // Simple markdown highlighting - NO syntax highlighting inside code
        const lines = text.split('\n');
        const result = [];
        let inCodeBlock = false;
        let codeBlockFence = '';
        
        for (let i = 0; i < lines.length; i++) {
            const line = lines[i];
            
            // Check for code block fence (opening or closing)
            const fenceMatch = line.match(/^(\s{0,3})(```+|~~~+)(\w[\w+-]*)?(\s*)$/);
            
            if (fenceMatch) {
                const fence = fenceMatch[2];
                
                if (!inCodeBlock) {
                    // Opening fence
                    inCodeBlock = true;
                    codeBlockFence = fence;
                    result.push(`<span class="hl-code-block">${escapeHtml(line)}</span>`);
                } else if (fence.startsWith(codeBlockFence[0]) && fence.length >= codeBlockFence.length) {
                    // Closing fence
                    result.push(`<span class="hl-code-block">${escapeHtml(line)}</span>`);
                    inCodeBlock = false;
                    codeBlockFence = '';
                } else {
                    // Inside code block, treat as code
                    result.push(escapeHtml(line));
                }
            } else if (inCodeBlock) {
                // Inside code block - wrap in span for coloring
                result.push(`<span class="hl-code-block">${escapeHtml(line)}</span>`);
            } else {
                // Outside code block - apply markdown formatting
                let processed = line;
                
                // First handle inline code to protect it from other replacements
                const codeSegments = [];
                let codeIndex = 0;
                processed = processed.replace(/`([^`]+)`/g, (match, code) => {
                    const placeholder = `__CODE_INLINE_${codeIndex}__`;
                    codeSegments[codeIndex] = `<span class="hl-code">${escapeHtml(match)}</span>`;
                    codeIndex++;
                    return placeholder;
                });
                
                // Now escape HTML
                processed = escapeHtml(processed);
                
                // Headers (must be at line start)
                if (/^#{1,6}\s+/.test(line)) {
                    processed = `<span class="hl-heading">${processed}</span>`;
                }
                // Blockquotes
                else if (/^>\s/.test(line)) {
                    processed = `<span class="hl-blockquote">${processed}</span>`;
                }
                // List markers
                else if (/^(\s*)[*+-]\s/.test(line) || /^(\s*)\d+\.\s/.test(line)) {
                    processed = processed.replace(/^(\s*)([*+-]|\d+\.)(\s)/, '$1<span class="hl-list-marker">$2</span>$3');
                }
                
                // Links [text](url)
                processed = processed.replace(/\[([^\]]+)\]\(([^)]+)\)/g, 
                    '<span class="hl-link">[$1]($2)</span>');
                
                // Bold **text** or __text__
                processed = processed.replace(/\*\*([^*]+)\*\*/g, 
                    '<span class="hl-bold">**$1**</span>');
                processed = processed.replace(/__([^_]+)__/g, 
                    '<span class="hl-bold">__$1__</span>');
                
                // Italic *text* or _text_ (avoid middle of words)
                processed = processed.replace(/(\s|^)\*([^*\s][^*]*)\*(\s|$)/g, 
                    '$1<span class="hl-italic">*$2*</span>$3');
                processed = processed.replace(/(\s|^)_([^_\s][^_]*)_(\s|$)/g, 
                    '$1<span class="hl-italic">_$2_</span>$3');
                
                // Restore inline code
                for (let j = 0; j < codeSegments.length; j++) {
                    processed = processed.replace(`__CODE_INLINE_${j}__`, codeSegments[j]);
                }
                
                result.push(processed);
            }
        }
        
        return result.join('\n');
    }
    
    tokenize(text, patterns) {
        const tokens = [];
        
        for (const pattern of patterns) {
            const regex = new RegExp(pattern.regex);
            let match;
            
            while ((match = regex.exec(text)) !== null) {
                // Handle grouped capture
                if (pattern.group !== undefined && match[pattern.group]) {
                    const groupStart = match.index + match[0].indexOf(match[pattern.group]);
                    tokens.push({
                        start: groupStart,
                        end: groupStart + match[pattern.group].length,
                        type: pattern.type,
                        wrap: pattern.wrap
                    });
                } else if (!pattern.special) {
                    tokens.push({
                        start: match.index,
                        end: match.index + match[0].length,
                        type: pattern.type
                    });
                }
            }
        }
        
        // Sort by start position, then by length (longer tokens first)
        tokens.sort((a, b) => {
            if (a.start !== b.start) return a.start - b.start;
            return (b.end - b.start) - (a.end - a.start);
        });
        
        // Remove overlapping tokens
        return this.removeOverlaps(tokens);
    }
    
    removeOverlaps(tokens) {
        const filtered = [];
        let lastEnd = 0;
        
        for (const token of tokens) {
            if (token.start >= lastEnd) {
                filtered.push(token);
                lastEnd = token.end;
            }
        }
        
        return filtered;
    }
    
    renderTokens(text, tokens) {
        let result = '';
        let pos = 0;
        
        for (const token of tokens) {
            if (pos < token.start) {
                result += escapeHtml(text.substring(pos, token.start));
            }
            
            const tokenText = text.substring(token.start, token.end);
            result += `<span class="hl-${token.type}">${escapeHtml(tokenText)}</span>`;
            pos = token.end;
        }
        
        if (pos < text.length) {
            result += escapeHtml(text.substring(pos));
        }
        
        return result;
    }
}