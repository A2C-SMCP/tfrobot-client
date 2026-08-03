import { Space, Typography } from 'antd';
import { ManagerAccount } from '@/components/ManagerAccount';

const { Title, Text } = Typography;

export function RobotConnections() {
  return (
    <Space direction="vertical" size={16} style={{ width: '100%' }}>
      <div>
        <Title level={3} style={{ marginBottom: 4 }}>
          Robot Connections
        </Title>
        <Text type="secondary">
          Manage Robot connection resources available to Computer instances.
        </Text>
      </div>

      <ManagerAccount />
    </Space>
  );
}
