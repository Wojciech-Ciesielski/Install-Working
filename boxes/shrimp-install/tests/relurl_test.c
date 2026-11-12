#include <assert.h>
#include "../rufus/src/shrimp/relurl.h"

int main(void)
{
	const char* j =
		"{\"url\":\"https://api.github.com/repos/o/r/releases/1\",\"name\":\"Shrimp Beta\",\"assets\":["
		"{\"url\":\"https://api.github.com/repos/o/r/releases/assets/11\",\"id\":11,\"name\":\"shrimp.iso\","
		"\"uploader\":{\"login\":\"x\",\"url\":\"https://api.github.com/users/x\"}},"
		"{\"url\":\"https://api.github.com/repos/o/r/releases/assets/22\",\"id\":22,\"name\":\"manifest.json\","
		"\"uploader\":{\"login\":\"x\",\"url\":\"https://api.github.com/users/x\"}},"
		"{\"url\":\"https://api.github.com/repos/o/r/releases/assets/33\",\"id\":33,\"name\":\"manifest.json.sig\"}]}";
	char u[256];
	assert(shrimp_release_asset_url(j, "manifest.json", u, sizeof(u)));
	assert(strcmp(u, "https://api.github.com/repos/o/r/releases/assets/22") == 0);
	assert(shrimp_release_asset_url(j, "manifest.json.sig", u, sizeof(u)));
	assert(strcmp(u, "https://api.github.com/repos/o/r/releases/assets/33") == 0);
	assert(shrimp_release_asset_url(j, "shrimp.iso", u, sizeof(u)));
	assert(strcmp(u, "https://api.github.com/repos/o/r/releases/assets/11") == 0);
	assert(!shrimp_release_asset_url(j, "nie-ma", u, sizeof(u)));
	assert(!shrimp_release_asset_url(j, "Shrimp Beta", u, sizeof(u)));
	assert(!shrimp_release_asset_url(j, "manifest.json", u, 10));
	puts("ok");
	return 0;
}
