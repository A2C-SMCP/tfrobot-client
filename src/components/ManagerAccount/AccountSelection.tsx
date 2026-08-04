import { Card, List, Button, Typography, Space, Alert, Tag } from 'antd';
import { UserOutlined, ArrowLeftOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { useManagerStore, type ManagerError } from '@/stores/managerStore';

const { Title, Text } = Typography;

function errorI18nKey(err: ManagerError): string {
  return `manager.errors.${err.kind}`;
}

interface AccountSelectionProps {
  embedded?: boolean;
}

export function AccountSelection({ embedded = false }: AccountSelectionProps) {
  const { t } = useTranslation();
  const { pendingAccountSelection, selectAccount, identityLoading, identityError, clearError, logout } =
    useManagerStore();

  if (!pendingAccountSelection) return null;

  const handleSelect = async (accountId: string) => {
    clearError();
    try {
      await selectAccount(accountId);
    } catch {
      /* error already stored */
    }
  };

  const content = (
    <Space direction="vertical" size="large" style={{ width: '100%' }}>
        <div>
          <Title level={4} style={{ marginBottom: 4 }}>
            {t('managerAccount.accountSelection.title')}
          </Title>
          <Text type="secondary">{t('managerAccount.accountSelection.description')}</Text>
        </div>

        {identityError && (
          <Alert
            type="error"
            showIcon
            message={t(errorI18nKey(identityError))}
            closable
            onClose={clearError}
          />
        )}

        <List
          bordered
          dataSource={pendingAccountSelection}
          rowKey={(account) => account.accountId}
          renderItem={(account) => (
            <List.Item
              actions={[
                <Button
                  key="select"
                  type="primary"
                  size="small"
                  loading={identityLoading}
                  onClick={() => handleSelect(account.accountId)}
                >
                  {t('managerAccount.accountSelection.select')}
                </Button>,
              ]}
            >
              <List.Item.Meta
                avatar={<UserOutlined />}
                title={account.nickname || account.accountName}
                description={
                  <Space size={4} wrap>
                    <Tag>{account.accountName}</Tag>
                    {account.organizationName && <Tag color="blue">{account.organizationName}</Tag>}
                    {account.organizationType && (
                      <Tag color={account.organizationType === 'enterprise' ? 'geekblue' : 'default'}>
                        {account.organizationType}
                      </Tag>
                    )}
                  </Space>
                }
              />
            </List.Item>
          )}
        />

        <Button icon={<ArrowLeftOutlined />} onClick={() => void logout()} block>
          {t('managerAccount.accountSelection.back')}
        </Button>
    </Space>
  );
  return embedded ? content : <Card>{content}</Card>;
}
