import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { createFactoryKit } from '@/audio/sampler/factoryKit';
import { KitMenu } from './KitMenu';

function setup() {
  const kit = { ...createFactoryKit(), id: 'classic-tr808', name: '808-style' };
  const props = {
    kitName: 'Factory',
    userKits: [],
    onLoadFactoryKit: vi.fn(),
    onLoadKit: vi.fn(),
    onSaveKit: vi.fn(),
    onBrowsePacks: vi.fn(),
    builtInKits: [{ id: 'classic:tr808', name: '808-style', color: 'red', create: () => kit }],
    packKits: [
      { id: 'pack:a:main', name: 'Fischer Basic', detail: 'TR-808 recordings', create: () => kit },
      { id: 'pack:gone:main', name: 'Broken', detail: 'Gone', create: () => null },
    ],
  };
  render(<KitMenu {...props} />);
  fireEvent.click(screen.getByRole('button', { name: 'Kit: Factory' }));
  return { props, kit };
}

describe('KitMenu', () => {
  it('lists built-in, installed-pack and saved kits', () => {
    setup();
    expect(screen.getByText('Built-in')).toBeInTheDocument();
    expect(screen.getByText('Installed packs')).toBeInTheDocument();
    expect(screen.getByText('Your kits')).toBeInTheDocument();
    expect(screen.getByText('TR-808 recordings')).toBeInTheDocument();
  });

  it('loads a built-in or pack kit', () => {
    const { props, kit } = setup();
    fireEvent.click(screen.getByRole('button', { name: /Fischer Basic/ }));
    expect(props.onLoadKit).toHaveBeenCalledWith(kit);
  });

  it('skips kits that cannot be built', () => {
    const { props } = setup();
    fireEvent.click(screen.getByRole('button', { name: /Broken/ }));
    expect(props.onLoadKit).not.toHaveBeenCalled();
  });

  it('opens the Packs page', () => {
    const { props } = setup();
    fireEvent.click(screen.getByRole('button', { name: 'Browse packs…' }));
    expect(props.onBrowsePacks).toHaveBeenCalled();
  });
});
