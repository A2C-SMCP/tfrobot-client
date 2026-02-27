import { render, screen } from '@testing-library/react';
import { Form } from 'antd';
import { SchemaForm } from '@/components/DebugPanel/SchemaForm';

function Wrapper({ schema }: { schema: Record<string, unknown> }) {
  const [form] = Form.useForm();
  return <SchemaForm schema={schema} form={form} />;
}

describe('SchemaForm', () => {
  it('renders string fields as text inputs', () => {
    const schema = {
      properties: {
        name: { type: 'string', description: 'Your name' },
      },
      required: ['name'],
    };

    render(<Wrapper schema={schema} />);

    expect(screen.getByText('name')).toBeInTheDocument();
  });

  it('renders number fields', () => {
    const schema = {
      properties: {
        count: { type: 'number' },
      },
    };

    render(<Wrapper schema={schema} />);

    expect(screen.getByText('count')).toBeInTheDocument();
  });

  it('renders boolean fields as switches', () => {
    const schema = {
      properties: {
        enabled: { type: 'boolean' },
      },
    };

    render(<Wrapper schema={schema} />);

    expect(screen.getByText('enabled')).toBeInTheDocument();
    expect(screen.getByRole('switch')).toBeInTheDocument();
  });

  it('renders enum fields as select dropdowns', () => {
    const schema = {
      properties: {
        color: { type: 'string', enum: ['red', 'blue', 'green'] },
      },
    };

    render(<Wrapper schema={schema} />);

    expect(screen.getByText('color')).toBeInTheDocument();
  });

  it('renders empty form for schema without properties', () => {
    const schema = {};

    const { container } = render(<Wrapper schema={schema} />);

    // Form should exist but have no form items
    expect(container.querySelectorAll('.ant-form-item')).toHaveLength(0);
  });

  it('renders multiple fields', () => {
    const schema = {
      properties: {
        host: { type: 'string' },
        port: { type: 'integer' },
        tls: { type: 'boolean' },
      },
      required: ['host'],
    };

    render(<Wrapper schema={schema} />);

    expect(screen.getByText('host')).toBeInTheDocument();
    expect(screen.getByText('port')).toBeInTheDocument();
    expect(screen.getByText('tls')).toBeInTheDocument();
  });
});
