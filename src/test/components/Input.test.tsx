import { render, screen } from '../helpers/render';
import { Input } from '@/components/common/Input';

// Guards the contract of the shared Input wrapper (issue #26): every text field
// opts out of WKWebView auto-capitalization / auto-correction / spell check /
// autofill by default, while still allowing explicit per-prop overrides.
describe('common/Input wrapper', () => {
  it('Input disables auto-correction and autofill by default', () => {
    render(<Input placeholder="cmd" />);
    const input = screen.getByPlaceholderText('cmd');
    expect(input).toHaveAttribute('autocapitalize', 'off');
    expect(input).toHaveAttribute('autocorrect', 'off');
    expect(input).toHaveAttribute('spellcheck', 'false');
    expect(input).toHaveAttribute('autocomplete', 'off');
  });

  it('Input.TextArea disables auto-correction by default', () => {
    render(<Input.TextArea placeholder="script" />);
    const area = screen.getByPlaceholderText('script');
    expect(area).toHaveAttribute('autocapitalize', 'off');
    expect(area).toHaveAttribute('autocorrect', 'off');
    expect(area).toHaveAttribute('spellcheck', 'false');
  });

  it('Input.Password disables auto-correction by default', () => {
    render(<Input.Password placeholder="secret" />);
    const pwd = screen.getByPlaceholderText('secret');
    expect(pwd).toHaveAttribute('autocapitalize', 'off');
    expect(pwd).toHaveAttribute('autocorrect', 'off');
    expect(pwd).toHaveAttribute('spellcheck', 'false');
  });

  it('lets callers override the defaults (e.g. password-manager autoComplete)', () => {
    render(<Input placeholder="user" autoComplete="username" />);
    expect(screen.getByPlaceholderText('user')).toHaveAttribute('autocomplete', 'username');
  });
});
