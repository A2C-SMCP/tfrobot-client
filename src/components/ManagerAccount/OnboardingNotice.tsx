import { Alert, Button } from 'antd';
import { ArrowLeftOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';

interface OnboardingNoticeProps {
  onBack: () => void;
  loading?: boolean;
  /** The page shell shows the back arrow; the header popover stays compact without it. */
  withBackIcon?: boolean;
}

/**
 * Onboarding copy is rendered both in the account popover and on the Manager page. Keeping it in
 * one component is what stops the two shells from drifting into different wording.
 */
export function OnboardingNotice({
  onBack,
  loading = false,
  withBackIcon = false,
}: OnboardingNoticeProps) {
  const { t } = useTranslation();

  return (
    <>
      <Alert
        type="info"
        showIcon
        message={t('managerAccount.onboarding.title')}
        description={t('managerAccount.onboarding.description')}
      />
      <Button
        icon={withBackIcon ? <ArrowLeftOutlined /> : undefined}
        loading={loading}
        onClick={onBack}
        block
      >
        {t('managerAccount.onboarding.back')}
      </Button>
    </>
  );
}
