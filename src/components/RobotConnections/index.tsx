import { Empty, Space, Typography } from 'antd';
import { LoginOutlined } from '@ant-design/icons';
import { EmployeeList } from '@/components/ManagerAccount/EmployeeList';
import { managerSessionFromContext, useManagerStore } from '@/stores/managerStore';
import { useTranslation } from 'react-i18next';

const { Title, Text } = Typography;

export function RobotConnections() {
  const { t } = useTranslation();
  const context = useManagerStore((state) => state.context);
  const session = managerSessionFromContext(context);
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

      {session ? (
        <EmployeeList showIdentityActions={false} />
      ) : (
        <Empty
          image={<LoginOutlined style={{ fontSize: 48 }} />}
          description={t('managerAccount.global.robotConnectionsSignIn')}
        />
      )}
    </Space>
  );
}
