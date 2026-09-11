import { Image as AntImage, Modal as AntModal, Popconfirm as AntPopconfirm, type ImageProps, type ModalProps, type PopconfirmProps } from 'antd';
import { useEffect, useState } from 'react';
import { usePageActive } from './pageActivityState';

/** Hide an editor without destroying its form or calling the user's discard action. */
export function PageModal(props: ModalProps) {
  const active = usePageActive();
  return <AntModal {...props} open={active && props.open}
    destroyOnHidden={active && props.destroyOnHidden}
    destroyOnClose={active && props.destroyOnClose}
    focusTriggerAfterClose={active && props.focusTriggerAfterClose !== false}
    onOk={(event) => { if (active) props.onOk?.(event); }} />;
}

/** Confirmations are transient intent, never saved with the page's draft. */
export function PagePopconfirm(props: PopconfirmProps) {
  const active = usePageActive();
  const [open, setOpen] = useState(false);
  useEffect(() => { if (!active) setOpen(false); }, [active]);
  return <AntPopconfirm key={String(active)} {...props} open={active && open}
    onOpenChange={(next, event) => { setOpen(active && next); props.onOpenChange?.(next, event); }}
    onConfirm={(event) => { if (active) return props.onConfirm?.(event); }} />;
}

export function PageImage(props: ImageProps) {
  const active = usePageActive();
  const [visible, setVisible] = useState(false);
  useEffect(() => { if (!active) setVisible(false); }, [active]);
  return <AntImage {...props} preview={props.preview === false ? false : {
    ...(typeof props.preview === 'object' ? props.preview : {}),
    visible: active && visible,
    onVisibleChange: setVisible,
  }} />;
}
