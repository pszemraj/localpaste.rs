#!/usr/bin/env node

// Simple script to test the frontend and report any errors
const puppeteer = require('puppeteer');

async function testFrontend() {
    console.log('🧪 Testing LocalPaste frontend...');
    
    let browser;
    try {
        browser = await puppeteer.launch({
            headless: 'new',
            args: ['--no-sandbox', '--disable-setuid-sandbox']
        });
        
        const page = await browser.newPage();
        
        // Collect console messages
        const logs = [];
        page.on('console', msg => {
            const type = msg.type();
            const text = msg.text();
            logs.push({ type, text });
            
            // Print colored output
            switch(type) {
                case 'error':
                    console.log(`🔴 ERROR: ${text}`);
                    break;
                case 'warning':
                    console.log(`🟡 WARNING: ${text}`);
                    break;
                case 'info':
                    if (text.includes('🚀') || text.includes('✅') || text.includes('📡')) {
                        console.log(`🔵 INFO: ${text}`);
                    }
                    break;
            }
        });
        
        // Collect page errors
        page.on('pageerror', error => {
            console.log(`🔴 PAGE ERROR: ${error.message}`);
            if (error.stack) {
                console.log(error.stack);
            }
        });
        
        // Navigate to the app
        console.log('📡 Navigating to http://localhost:3030...');
        await page.goto('http://localhost:3030', {
            waitUntil: 'networkidle2',
            timeout: 10000
        });
        
        // Wait a bit for JavaScript to initialize
        await page.waitForTimeout(2000);
        
        // Check if the app initialized
        const hasEditor = await page.$('#editor') !== null;
        const hasPasteList = await page.$('#paste-list') !== null;
        
        console.log(`\n📊 Status Check:`);
        console.log(`  Editor found: ${hasEditor ? '✅' : '❌'}`);
        console.log(`  Paste list found: ${hasPasteList ? '✅' : '❌'}`);
        
        // Try to create a new paste
        const newPasteBtn = await page.$('#new-paste-btn');
        if (newPasteBtn) {
            console.log('\n🧪 Testing new paste creation...');
            await newPasteBtn.click();
            await page.waitForTimeout(1000);
        }
        
        // Summary
        const errors = logs.filter(l => l.type === 'error');
        const warnings = logs.filter(l => l.type === 'warning');
        
        console.log(`\n📈 Summary:`);
        console.log(`  Total errors: ${errors.length}`);
        console.log(`  Total warnings: ${warnings.length}`);
        
        if (errors.length > 0) {
            console.log('\n❌ Frontend has errors! Please fix them.');
            process.exit(1);
        } else {
            console.log('\n✅ Frontend loaded successfully!');
        }
        
    } catch (error) {
        console.error('🔴 Test failed:', error);
        process.exit(1);
    } finally {
        if (browser) {
            await browser.close();
        }
    }
}

// Check if puppeteer is installed
try {
    require.resolve('puppeteer');
    testFrontend();
} catch (e) {
    console.log('⚠️  Puppeteer not installed. Install with: npm install puppeteer');
    console.log('   Or use curl to test manually: curl http://localhost:3030');
}