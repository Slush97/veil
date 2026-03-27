import { useAppStore } from '../../store/appStore';
import { ServerStrip } from './ServerStrip';
import { ChannelSidebar } from './ChannelSidebar';
import { ChatArea } from './ChatArea';
import { MemberList } from './MemberList';
import styles from './MainLayout.module.css';

export function MainLayout() {
  const memberListOpen = useAppStore((s) => s.ui.memberListOpen);

  return (
    <div className={styles.layout}>
      <ServerStrip />
      <ChannelSidebar />
      <ChatArea />
      {memberListOpen && <MemberList />}
    </div>
  );
}
