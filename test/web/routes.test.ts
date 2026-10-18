import assert from 'node:assert/strict';
import test from 'node:test';
import { postUrl, pageUrl } from '../../apps/web/src/lib/api';

test('article URLs separate authors from local article IDs', () => {
  assert.equal(postUrl('master~development'), '/posts/master/development');
  assert.equal(postUrl('other~development'), '/posts/other/development');
  assert.equal(postUrl('name_1-test~development'), '/posts/name_1-test/development');
  assert.equal(postUrl('api~development'), '/posts/api/development');
  assert.equal(postUrl('git~development'), '/posts/git/development');
  assert.throws(() => postUrl('../bad~development'));
});
test('multi-page links retain entry URLs and encode document names', () => {
  assert.equal(pageUrl('master~development', 'main.md', 'main.md'), '/posts/master/development');
  assert.equal(pageUrl('master~development', 'chapter.md', 'main.md'), '/posts/master/development?page=chapter');
  assert.equal(pageUrl('master~development', 'chapter.markdown', 'main.md'), '/posts/master/development?page=chapter');
  assert.equal(pageUrl('master~development', 'intro.txt', 'intro.txt'), '/posts/master/development');
  assert.equal(pageUrl('master~development', 'main.md'), '/posts/master/development');
});
