## vecfile 0.3.0 10/14/2019
 - Properly implemented Clone
 	- Clone couldn't originally be implemented because reads required mutability, but this was changed in 0.2.0
	- Developer's Note: This is a relatively small change, but Clone is an important trait to have impl for collections so I felt that this needed to be pushed out as soon as possible.

## vecfile 0.2.0 10/14/2019
 - All read-type operations can now be called on immutable VecFile instances, including iteration
