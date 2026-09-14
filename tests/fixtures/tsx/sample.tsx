import React from 'react'

/** Renders the user card with a seamless experience. */
export function UserCard({ userName, onSelect }: Props) {
  const [isOpen, setIsOpen] = React.useState(false)
  return (
    <div className="card container" title="Open the user card" onClick={() => onSelect(userName)}>
      {/* JSX comment about the header */}
      <img src="/static/avatar.png" alt="Avatar of the user" />
      <span>{isOpen ? 'Currently open' : 'Closed'}</span>
    </div>
  )
}
