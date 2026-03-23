import { describe, it, expect } from 'vitest';
import { renderMarkdown } from '../panels/chat/markdown';

describe('renderMarkdown', () => {
  it('returns empty string for empty input', () => {
    expect(renderMarkdown('')).toBe('');
  });

  it('escapes HTML entities to prevent XSS', () => {
    const result = renderMarkdown('<script>alert("xss")</script>');
    expect(result).not.toContain('<script>');
    expect(result).toContain('&lt;script&gt;');
  });

  describe('code blocks', () => {
    it('renders fenced code blocks with <pre><code>', () => {
      const input = '```\nconst x = 1;\n```';
      const result = renderMarkdown(input);
      expect(result).toContain('<pre><code>');
      expect(result).toContain('const x = 1;');
      expect(result).toContain('</code></pre>');
    });

    it('renders fenced code blocks with language class', () => {
      const input = '```rust\nfn main() {}\n```';
      const result = renderMarkdown(input);
      expect(result).toContain('class="language-rust"');
    });

    it('preserves code block content without inline transforms', () => {
      const input = '```\n**not bold** `not code`\n```';
      const result = renderMarkdown(input);
      // Inside code blocks, bold markers should remain as-is (escaped)
      expect(result).toContain('**not bold**');
    });
  });

  describe('inline code', () => {
    it('renders inline code with <code> tags', () => {
      const result = renderMarkdown('Use `foo()` here');
      expect(result).toContain('<code>foo()</code>');
    });
  });

  describe('bold', () => {
    it('renders **bold** as <strong>', () => {
      const result = renderMarkdown('This is **bold** text');
      expect(result).toContain('<strong>bold</strong>');
    });
  });

  describe('italic', () => {
    it('renders *italic* as <em>', () => {
      const result = renderMarkdown('This is *italic* text');
      expect(result).toContain('<em>italic</em>');
    });
  });

  describe('headers', () => {
    it('renders # as <h1>', () => {
      const result = renderMarkdown('# Title');
      expect(result).toContain('<h1>Title</h1>');
    });

    it('renders ## as <h2>', () => {
      const result = renderMarkdown('## Subtitle');
      expect(result).toContain('<h2>Subtitle</h2>');
    });

    it('renders ### as <h3>', () => {
      const result = renderMarkdown('### Section');
      expect(result).toContain('<h3>Section</h3>');
    });
  });

  describe('links', () => {
    it('renders [text](url) as <a> tags', () => {
      const result = renderMarkdown('[click here](https://example.com)');
      expect(result).toContain('<a href="https://example.com"');
      expect(result).toContain('>click here</a>');
    });

    it('adds target="_blank" and rel="noopener noreferrer"', () => {
      const result = renderMarkdown('[link](https://example.com)');
      expect(result).toContain('target="_blank"');
      expect(result).toContain('rel="noopener noreferrer"');
    });
  });

  describe('unordered lists', () => {
    it('renders - items as <ul><li>', () => {
      const input = '- item one\n- item two\n- item three';
      const result = renderMarkdown(input);
      expect(result).toContain('<ul>');
      expect(result).toContain('<li>item one</li>');
      expect(result).toContain('<li>item two</li>');
      expect(result).toContain('<li>item three</li>');
      expect(result).toContain('</ul>');
    });
  });

  describe('combined', () => {
    it('handles mixed markdown in a single input', () => {
      const input = '# Hello\n\nThis is **bold** and *italic*.\n\n```js\nconst x = 1;\n```\n\n- one\n- two';
      const result = renderMarkdown(input);
      expect(result).toContain('<h1>Hello</h1>');
      expect(result).toContain('<strong>bold</strong>');
      expect(result).toContain('<em>italic</em>');
      expect(result).toContain('<pre><code');
      expect(result).toContain('<li>one</li>');
      expect(result).toContain('<li>two</li>');
    });
  });
});
