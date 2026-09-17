import { invoke } from '@tauri-apps/api/core';
import { useSkillStore, type MarketplaceGovernance, type MarketplaceSource } from '../../src/stores/skillStore';

function check(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}
async function run() {
  const fixture = await invoke<{ remoteUrl: string; localPath: string }>('acceptance_fixture');
  const instanceId = 'marketplace-acceptance';
  const observations: string[] = [];
  const sources: MarketplaceSource[] = [
    { type: 'remoteGit', gitUrl: fixture.remoteUrl },
    { type: 'localGit', path: fixture.localPath },
  ];
  for (const [index, source] of sources.entries()) {
    const name = `market-${index}`;
    await useSkillStore.getState().addMarketplace(instanceId, { name, source });
    const before = await invoke<MarketplaceGovernance>('get_marketplace_governance', { instanceId });
    const row = before.marketplaces.find(item => item.name === name);
    check(row, `${source.type}: added marketplace absent`);
    check(before.plugins.some(item => item.marketplace === name && item.plugin === 'audit'), 'Cloned catalog not loaded');
    check(!useSkillStore.getState().marketplaceError, 'Store governance refresh failed');
    if (source.type === 'remoteGit') {
      check(row.source.type === 'remoteGit' && row.source.displayGitUrl === fixture.remoteUrl, 'displayGitUrl contract mismatch');
      check(!('display_git_url' in row.source), 'snake_case summary leaked');
    } else {
      check(row.source.type === 'localGit' && row.source.path === fixture.localPath, 'Local source mismatch');
    }
    observations.push(`${source.type}: add and catalog read PASS`);
    // Request a genuinely different source. Rejection must preserve the original source/catalog.
    const replacement = sources[1 - index];
    let failure: unknown;
    try {
      await useSkillStore.getState().updateMarketplace(instanceId, { name, source: replacement });
    } catch (error) { failure = error; }
    check(typeof failure === 'string' && failure.includes('atomic update API'), 'Update did not reach business restriction');
    const after = await invoke<MarketplaceGovernance>('get_marketplace_governance', { instanceId });
    check(JSON.stringify(after) === JSON.stringify(before), 'Rejected update changed governance');
    observations.push(`${replacement.type}: update parsed and rejected without mutation PASS`);
  }
  await invoke('acceptance_result', { passed: true, observations });
}
run().catch(async error => {
  document.getElementById('status')!.textContent = 'FAIL';
  await invoke('acceptance_result', { passed: false, observations: [String(error)] });
});
