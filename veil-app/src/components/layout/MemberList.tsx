import clsx from 'clsx';
import { Avatar, StatusDot } from '../common';
import { useAppStore } from '../../store/appStore';
import type { Member, Role } from '../../types';
import styles from './MemberList.module.css';

const ROLE_ORDER: Role[] = ['owner', 'admin', 'moderator', 'member'];
const ROLE_LABELS: Record<Role, string> = {
  owner: 'Owner',
  admin: 'Admin',
  moderator: 'Moderator',
  member: 'Members',
};

export function MemberList() {
  const members = useAppStore((s) => s.members);

  // Group by role, then sort: online members first within each role
  const grouped = ROLE_ORDER
    .map((role) => ({
      role,
      members: members
        .filter((m) => m.role === role)
        .sort((a, b) => {
          const statusOrder = { online: 0, idle: 1, dnd: 2, offline: 3 };
          return statusOrder[a.status] - statusOrder[b.status];
        }),
    }))
    .filter((g) => g.members.length > 0);

  return (
    <div className={styles.memberList}>
      {grouped.map((group) => (
        <div key={group.role}>
          <div className={styles.roleHeader}>
            {ROLE_LABELS[group.role]} — {group.members.length}
          </div>
          {group.members.map((member) => (
            <MemberRow key={member.peerId} member={member} />
          ))}
        </div>
      ))}
    </div>
  );
}

function MemberRow({ member }: { member: Member }) {
  return (
    <div className={clsx(styles.memberRow, member.status === 'offline' && styles.offline)}>
      <div className={styles.memberAvatar}>
        <Avatar name={member.displayName} size="md" />
        <StatusDot status={member.status} positioned />
      </div>
      <div className={styles.memberInfo}>
        <div className={clsx(styles.memberName, member.status === 'offline' && styles.offline)}>
          {member.displayName}
          {member.isSelf && <span className={styles.selfTag}>(you)</span>}
        </div>
        {member.isTyping ? (
          <div className={styles.typingTag}>typing...</div>
        ) : member.statusText ? (
          <div className={styles.memberStatus}>{member.statusText}</div>
        ) : null}
      </div>
    </div>
  );
}
