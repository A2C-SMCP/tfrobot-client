import { Card, Space, Typography } from 'antd';
import { CodeOutlined, SafetyCertificateOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import type { ComputerInstance } from '@/stores/computerStore';
import { CommandLineToolSettings } from './CommandLineToolSettings';
import { RemoteControlSettings } from './RemoteControlSettings';

const { Paragraph, Title } = Typography;

interface BuiltInToolsSettingsProps {
  instance: ComputerInstance;
}

export function BuiltInToolsSettings({ instance }: BuiltInToolsSettingsProps) {
  const { t } = useTranslation();
  return (
    <Space direction="vertical" size="large" style={{ width: '100%' }}>
      <Card>
        <Space direction="vertical" size="middle" style={{ width: '100%' }}>
          <div>
            <Title level={5} style={{ margin: 0 }}>
              <SafetyCertificateOutlined />{' '}
              {t('computer.builtInTools.robotControl.title')}
            </Title>
            <Paragraph type="secondary">
              {t('computer.builtInTools.robotControl.description')}
            </Paragraph>
          </div>
          <RemoteControlSettings instance={instance} />
        </Space>
      </Card>
      <Card>
        <Space direction="vertical" size="middle" style={{ width: '100%' }}>
          <div>
            <Title level={5} style={{ margin: 0 }}>
              <CodeOutlined /> {t('computer.builtInTools.commandLine.title')}
            </Title>
            <Paragraph type="secondary">
              {t('computer.builtInTools.commandLine.description')}
            </Paragraph>
          </div>
          <CommandLineToolSettings computerId={instance.id} />
        </Space>
      </Card>
    </Space>
  );
}
