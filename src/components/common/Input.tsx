import { Input as AntInput, type GetProps, type GetRef } from 'antd';
import { forwardRef } from 'react';

/**
 * Drop-in replacement for antd's `Input` that disables macOS WKWebView's
 * system-level text substitution (auto-capitalization, auto-correction, spell
 * check, autofill history). On macOS the WebView rewrites technical input —
 * e.g. typing `npx` becomes `Npx`, which then fails to spawn (issue #26). These
 * defaults are safe across this technical desktop app and remain overridable:
 * callers can still pass an explicit `autoComplete` (e.g. the login form's
 * `username` / `current-password`) and it wins via the spread order.
 */
const noAutoCorrect = {
  autoCapitalize: 'off',
  autoCorrect: 'off',
  spellCheck: false,
} as const;

type InputProps = GetProps<typeof AntInput>;
type TextAreaProps = GetProps<typeof AntInput.TextArea>;
type PasswordProps = GetProps<typeof AntInput.Password>;
type InputRef = GetRef<typeof AntInput>;
type TextAreaRef = GetRef<typeof AntInput.TextArea>;

const InputBase = forwardRef<InputRef, InputProps>(function Input(props, ref) {
  return <AntInput ref={ref} autoComplete="off" {...noAutoCorrect} {...props} />;
});

const TextArea = forwardRef<TextAreaRef, TextAreaProps>(function TextArea(props, ref) {
  return <AntInput.TextArea ref={ref} {...noAutoCorrect} {...props} />;
});

const Password = forwardRef<InputRef, PasswordProps>(function Password(props, ref) {
  return <AntInput.Password ref={ref} {...noAutoCorrect} {...props} />;
});

type InputComponent = typeof InputBase & {
  TextArea: typeof TextArea;
  Password: typeof Password;
};

const Input = InputBase as InputComponent;
Input.TextArea = TextArea;
Input.Password = Password;

export { Input };
